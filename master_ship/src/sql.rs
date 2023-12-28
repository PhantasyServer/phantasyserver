use crate::Error;
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use data_structs::AccountStorages;
use pso2packetlib::{
    protocol::login::{LoginAttempt, LoginResult, UserInfoPacket},
    AsciiString,
};
use rand_core::{OsRng, RngCore};
use sqlx::{migrate::MigrateDatabase, Connection, Executor, Row};
use std::{
    net::Ipv4Addr,
    ops::Add,
    str::from_utf8,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub struct Sql {
    connection: sqlx::AnyPool,
}

pub struct User {
    pub id: u32,
    pub nickname: String,
}

impl Sql {
    pub async fn new(path: &str) -> Result<Self, Error> {
        sqlx::any::install_default_drivers();
        if !sqlx::Any::database_exists(path).await.unwrap_or(false) {
            return Self::create_db(path).await;
        }
        let conn = sqlx::AnyPool::connect(path).await?;
        Ok(Self { connection: conn })
    }
    async fn create_db(path: &str) -> Result<Self, Error> {
        sqlx::Any::create_database(path).await?;
        let auto_inc = match sqlx::AnyConnection::connect(path).await?.backend_name() {
            "SQLite" => "autoincrement",
            _ => "auto_increment",
        };
        let conn = sqlx::AnyPool::connect(path).await?;
        conn.execute(
            format!(
                "
            create table if not exists Users (
                Id integer primary key {},
                Username blob default NULL,
                Nickname blob default NULL,
                Password blob default NULL,
                PSNNickname blob default NULL,
                Settings blob default NULL,
                Storage blob default NULL,
                Info blob default NULL
            );
        ",
                auto_inc
            )
            .as_str(),
        )
        .await?;
        conn.execute(
            format!(
                "
            create table if not exists Logins (
                Id integer primary key {},
                UserId integer default NULL,
                IpAddress blob default NULL,
                Status blob default NULL,
                Timestamp integer default NULL
            );
        ",
                auto_inc
            )
            .as_str(),
        )
        .await?;
        conn.execute(
            "
            create table if not exists Challenges (
                UserId integer default 0,
                Challenge integer default 0,
                Until integer default 0
            );
        ",
        )
        .await?;
        Ok(Self { connection: conn })
    }
    pub async fn get_sega_user(
        &self,
        username: &str,
        password: &str,
        ip: Ipv4Addr,
    ) -> Result<User, Error> {
        if username.is_empty() || password.is_empty() {
            return Err(Error::InvalidData);
        }
        let row = sqlx::query("select * from Users where Username = ?")
            .bind(username.as_bytes())
            .fetch_optional(&self.connection)
            .await?;
        match row {
            Some(data) => {
                let stored_password = from_utf8(data.try_get("Password")?)?;
                let id = data.try_get::<i64, _>("Id")? as u32;
                let nickname = from_utf8(data.try_get("Nickname").unwrap_or_default())?;
                // SAFETY: reference doesn't outlive the scope because the thread is immediately
                // joined
                let stored_password: &'static str = unsafe { std::mem::transmute(stored_password) };
                // SAFETY: same as above
                let password: &'static [u8] = unsafe { std::mem::transmute(password.as_bytes()) };

                match tokio::task::spawn_blocking(move || -> Result<(), Error> {
                    let hash = match PasswordHash::new(stored_password) {
                        Ok(x) => x,
                        Err(_) => return Err(Error::InvalidPassword(id)),
                    };
                    match Argon2::default().verify_password(password, &hash) {
                        Ok(_) => Ok(()),
                        Err(_) => Err(Error::InvalidPassword(id)),
                    }
                })
                .await
                .unwrap()
                {
                    Ok(_) => {}
                    Err(e) => {
                        self.put_login(id, ip, LoginResult::LoginError).await?;
                        return Err(e);
                    }
                }
                self.put_login(id, ip, LoginResult::Successful).await?;
                Ok(User {
                    id,
                    nickname: nickname.to_string(),
                })
            }
            None => Err(Error::NoUser),
        }
    }
    pub async fn get_user_info(&self, user_id: u32) -> Result<UserInfoPacket, Error> {
        let row = sqlx::query("select * from Users where Id = ?")
            .bind(user_id as i64)
            .fetch_optional(&self.connection)
            .await?;
        if row.is_none() {
            return Err(Error::NoUser);
        }
        Ok(
            serde_json::from_slice(row.unwrap().try_get("Info").unwrap_or_default())
                .unwrap_or_default(),
        )
    }
    pub async fn put_user_info(&self, user_id: u32, info: UserInfoPacket) -> Result<(), Error> {
        sqlx::query("update Users set Info = ? where Id = ?")
            .bind(serde_json::to_vec(&info)?)
            .bind(user_id as i64)
            .execute(&self.connection)
            .await?;
        Ok(())
    }
    pub async fn new_challenge(&self, user_id: u32) -> Result<u32, Error> {
        let row = sqlx::query("select * from Users where Id = ?")
            .bind(user_id as i64)
            .fetch_optional(&self.connection)
            .await?;
        if row.is_none() {
            return Err(Error::NoUser);
        }
        let challenge = OsRng.next_u32();
        let until = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .add(Duration::from_secs(60))
            .as_secs();
        sqlx::query("insert into Challenges (UserId, Challenge, Until) values (?, ?, ?)")
            .bind(user_id as i64)
            .bind(challenge as i64)
            .bind(until as i64)
            .execute(&self.connection)
            .await?;
        Ok(challenge)
    }
    pub async fn drop_challenges(&self) -> Result<(), Error> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        sqlx::query("delete from Challenges where Until < ?")
            .bind(now as i64)
            .execute(&self.connection)
            .await?;
        Ok(())
    }
    pub async fn login_challenge(&self, user_id: u32, challenge: u32) -> Result<User, Error> {
        self.drop_challenges().await?;
        let rows = sqlx::query("select * from Challenges where (UserId = ? and Challenge = ?)")
            .bind(user_id as i64)
            .bind(challenge as i64)
            .fetch_all(&self.connection)
            .await?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        for row in rows {
            let until = row.try_get::<i64, _>("Until")? as u64;
            if until < now {
                continue;
            }
            let row = sqlx::query("select * from Users where Id = ?")
                .bind(user_id as i64)
                .fetch_one(&self.connection)
                .await?;
            let nickname = from_utf8(row.try_get("Nickname").unwrap_or_default())?;
            return Ok(User {
                id: user_id,
                nickname: nickname.to_string(),
            });
        }
        Err(Error::NoUser)
    }
    pub async fn get_psn_user(&self, username: &str, ip: Ipv4Addr) -> Result<User, Error> {
        if username.is_empty() {
            return Err(Error::InvalidData);
        }
        let row = sqlx::query("select * from Users where PSNNickname = ?")
            .bind(username.as_bytes())
            .fetch_optional(&self.connection)
            .await?;
        match row {
            Some(data) => {
                let id = data.try_get::<i64, _>("Id")? as u32;
                let nickname = from_utf8(data.try_get("Nickname").unwrap_or_default())?;
                self.put_login(id, ip, LoginResult::Successful).await?;
                Ok(User {
                    id,
                    nickname: nickname.to_string(),
                })
            }
            None => Err(Error::NoUser),
        }
    }
    pub async fn create_psn_user(&self, username: &str) -> Result<User, Error> {
        sqlx::query("insert into Users (PSNNickname, Settings) values (?, ?)")
            .bind(username.as_bytes())
            .bind("".as_bytes())
            .execute(&self.connection)
            .await?;
        let id = sqlx::query("select Id from Users where PSNNickname = ?")
            .bind(username.as_bytes())
            .fetch_one(&self.connection)
            .await?
            .try_get::<i64, _>("Id")? as u32;
        Ok(User {
            id,
            nickname: String::new(),
        })
    }
    pub async fn create_sega_user(&self, username: &str, password: &str) -> Result<User, Error> {
        // SAFETY: references do not outlive the scope because the thread is immediately
        // joined
        let password: &'static str = unsafe { std::mem::transmute(password) };
        let hash = tokio::task::spawn_blocking(|| {
            let salt = SaltString::generate(&mut OsRng);
            let argon2 = Argon2::default();
            match argon2.hash_password(password.as_bytes(), &salt) {
                Ok(x) => Ok(x.to_string()),
                Err(_) => Err(Error::HashError),
            }
        })
        .await
        .unwrap()?;
        sqlx::query("insert into Users (Username, Password, Settings) values (?, ?, ?)")
            .bind(username.as_bytes())
            .bind(hash.as_bytes())
            .bind("".as_bytes())
            .execute(&self.connection)
            .await?;
        let id = sqlx::query("select Id from Users where Username = ?")
            .bind(username.as_bytes())
            .fetch_one(&self.connection)
            .await?
            .try_get::<i64, _>("Id")? as u32;
        Ok(User {
            id,
            nickname: String::new(),
        })
    }
    pub async fn get_logins(&self, id: u32) -> Result<Vec<LoginAttempt>, Error> {
        let mut attempts = vec![];
        let rows =
            sqlx::query("select * from Logins where UserId = ? order by Timestamp desc limit 50")
                .bind(id as i64)
                .fetch_all(&self.connection)
                .await?;
        for row in rows {
            attempts.push(LoginAttempt {
                ip: serde_json::from_str(from_utf8(row.try_get("IpAddress")?)?)?,
                status: serde_json::from_str(from_utf8(row.try_get("Status")?)?)?,
                timestamp: Duration::from_secs(row.try_get::<i64, _>("Timestamp")? as u64),
                ..Default::default()
            })
        }
        Ok(attempts)
    }
    pub async fn put_login(&self, id: u32, ip: Ipv4Addr, status: LoginResult) -> Result<(), Error> {
        let timestamp_int = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        sqlx::query(
            "insert into Logins (UserId, IpAddress, Status, Timestamp) values (?, ?, ?, ?)",
        )
        .bind(id as i64)
        .bind(serde_json::to_string(&ip)?.as_bytes())
        .bind(serde_json::to_string(&status)?.as_bytes())
        .bind(timestamp_int as i64)
        .execute(&self.connection)
        .await?;
        Ok(())
    }
    pub async fn get_account_storage(&self, user_id: u32) -> Result<AccountStorages, Error> {
        let row = sqlx::query("select Storage from Users where Id = ?")
            .bind(user_id as i64)
            .fetch_one(&self.connection)
            .await?;
        match row.try_get("Storage") {
            Ok(d) => Ok(serde_json::from_str(from_utf8(d)?)?),
            Err(_) => Ok(Default::default()),
        }
    }
    pub async fn put_account_storage(
        &self,
        user_id: u32,
        storage: AccountStorages,
    ) -> Result<(), Error> {
        sqlx::query("update Users set Storage = ? where Id = ?")
            .bind(serde_json::to_string(&storage)?.as_bytes())
            .bind(user_id as i64)
            .execute(&self.connection)
            .await?;
        Ok(())
    }
    pub async fn get_settings(&self, id: u32) -> Result<AsciiString, Error> {
        let row = sqlx::query("select Settings from Users where Id = ?")
            .bind(id as i64)
            .fetch_optional(&self.connection)
            .await?;
        match row {
            Some(data) => Ok(from_utf8(data.try_get("Settings")?)?.into()),
            None => Ok(Default::default()),
        }
    }
    pub async fn save_settings(&self, id: u32, settings: &str) -> Result<(), Error> {
        sqlx::query("update Users set Settings = ? where Id = ?")
            .bind(settings.as_bytes())
            .bind(id as i64)
            .execute(&self.connection)
            .await?;
        Ok(())
    }
}
