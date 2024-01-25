use crate::Error;
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use data_structs::{flags::Flags, inventory::AccountStorages};
use pso2packetlib::{
    protocol::login::{LoginAttempt, LoginResult, UserInfoPacket},
    AsciiString,
};
use rand_core::{OsRng, RngCore};
use sqlx::{migrate::MigrateDatabase, Executor, Row};
use std::{
    net::Ipv4Addr,
    ops::Add,
    str::from_utf8,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub struct Sql {
    connection: sqlx::SqlitePool,
    registration_enabled: bool,
}

#[derive(PartialEq, Debug)]
pub struct User {
    pub id: u32,

    pub nickname: String,
    pub account_flags: Flags,
    pub isgm: bool,
    pub last_uuid: u64,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct UserData {
    nickname: String,
    settings: String,
    storage: AccountStorages,
    info: UserInfoPacket,
    flags: Flags,
    isgm: bool,
    last_uuid: u64,
}

impl Sql {
    pub async fn new(path: &str, reg_enabled: bool) -> Result<Self, Error> {
        if !sqlx::Sqlite::database_exists(path).await.unwrap_or(false) {
            return Self::create_db(path, reg_enabled).await;
        }
        let conn = sqlx::SqlitePool::connect(path).await?;
        Ok(Self {
            connection: conn,
            registration_enabled: reg_enabled,
        })
    }
    async fn create_db(path: &str, reg_enabled: bool) -> Result<Self, Error> {
        sqlx::Sqlite::create_database(path).await?;
        let conn = sqlx::SqlitePool::connect(path).await?;
        conn.execute(
            "
            create table if not exists Users (
                Id integer primary key autoincrement,
                Username blob,
                Password blob,
                PSNUsername blob,
                Data blob
            );
        ",
        )
        .await?;
        conn.execute(
            "
            create table if not exists Logins (
                Id integer primary key autoincrement,
                UserId integer default NULL,
                IpAddress blob default NULL,
                Status blob default NULL,
                Timestamp integer default NULL
            );
        ",
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
        conn.execute(
            "
            create table if not exists Ships (
                PSK blob
            );
        ",
        )
        .await?;
        Ok(Self {
            connection: conn,
            registration_enabled: reg_enabled,
        })
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
                let user_data: UserData = rmp_serde::from_slice(data.try_get("Data")?)?;
                Ok(User {
                    id,
                    nickname: user_data.nickname,
                    account_flags: user_data.flags,
                    isgm: user_data.isgm,
                    last_uuid: user_data.last_uuid,
                })
            }
            None => Err(Error::NoUser),
        }
    }
    pub async fn get_user_info(&self, user_id: u32) -> Result<UserInfoPacket, Error> {
        let Some(row) = sqlx::query("select * from Users where Id = ?")
            .bind(user_id as i64)
            .fetch_optional(&self.connection)
            .await?
        else {
            return Err(Error::NoUser);
        };
        let user_data: UserData = rmp_serde::from_slice(row.try_get("Data")?)?;
        Ok(user_data.info)
    }
    pub async fn put_user_info(&self, user_id: u32, info: UserInfoPacket) -> Result<(), Error> {
        self.update_userdata(user_id, |user_data| user_data.info = info)
            .await
    }
    pub async fn put_account_flags(&self, user_id: u32, flags: Flags) -> Result<(), Error> {
        self.update_userdata(user_id, |user_data| user_data.flags = flags)
            .await
    }
    pub async fn new_challenge(&self, user_id: u32) -> Result<u32, Error> {
        if sqlx::query("select * from Users where Id = ?")
            .bind(user_id as i64)
            .fetch_optional(&self.connection)
            .await?
            .is_none()
        {
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
            let user_data: UserData = rmp_serde::from_slice(row.try_get("Data")?)?;
            return Ok(User {
                id: user_id,
                nickname: user_data.nickname,
                account_flags: user_data.flags,
                isgm: user_data.isgm,
                last_uuid: user_data.last_uuid,
            });
        }
        Err(Error::NoUser)
    }
    pub async fn get_psn_user(&self, username: &str, ip: Ipv4Addr) -> Result<User, Error> {
        if username.is_empty() {
            return Err(Error::InvalidData);
        }
        let row = sqlx::query("select * from Users where PSNUsername = ?")
            .bind(username.as_bytes())
            .fetch_optional(&self.connection)
            .await?;
        match row {
            Some(data) => {
                let id = data.try_get::<i64, _>("Id")? as u32;
                let user_data: UserData = rmp_serde::from_slice(data.try_get("Data")?)?;
                self.put_login(id, ip, LoginResult::Successful).await?;
                Ok(User {
                    id,
                    nickname: user_data.nickname,
                    account_flags: user_data.flags,
                    isgm: user_data.isgm,
                    last_uuid: user_data.last_uuid,
                })
            }
            None => Err(Error::NoUser),
        }
    }
    pub async fn create_psn_user(&self, username: &str) -> Result<User, Error> {
        let mut transaction = self.connection.begin().await?;
        let user_data = UserData {
            last_uuid: 1,
            ..Default::default()
        };
        let id = sqlx::query(
            "insert into Users (Username, Password, PSNUsername, Data) values (?, ?, ?, ?) 
            returning Id",
        )
        .bind(&b""[..])
        .bind(&b""[..])
        .bind(username.as_bytes())
        .bind(rmp_serde::to_vec(&user_data)?)
        .fetch_one(&mut *transaction)
        .await?
        .try_get::<i64, _>("Id")? as u32;
        transaction.commit().await?;

        Ok(User {
            id,
            nickname: user_data.nickname,
            account_flags: user_data.flags,
            isgm: user_data.isgm,
            last_uuid: user_data.last_uuid,
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

        let mut transaction = self.connection.begin().await?;
        let user_data = UserData {
            last_uuid: 1,
            ..Default::default()
        };
        let id = sqlx::query(
            "insert into Users (Username, Password, PSNUsername, Data) values (?, ?, ?, ?) 
            returning Id",
        )
        .bind(username.as_bytes())
        .bind(hash.as_bytes())
        .bind(&b""[..])
        .bind(rmp_serde::to_vec(&user_data)?)
        .fetch_one(&mut *transaction)
        .await?
        .try_get::<i64, _>("Id")? as u32;
        transaction.commit().await?;

        Ok(User {
            id,
            nickname: user_data.nickname,
            account_flags: user_data.flags,
            isgm: user_data.isgm,
            last_uuid: user_data.last_uuid,
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
                ip: rmp_serde::from_slice(row.try_get("IpAddress")?)?,
                status: rmp_serde::from_slice(row.try_get("Status")?)?,
                timestamp: Duration::from_secs(row.try_get::<i64, _>("Timestamp")? as u64),
                ..Default::default()
            })
        }
        Ok(attempts)
    }
    async fn put_login(&self, id: u32, ip: Ipv4Addr, status: LoginResult) -> Result<(), Error> {
        let timestamp_int = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        sqlx::query(
            "insert into Logins (UserId, IpAddress, Status, Timestamp) values (?, ?, ?, ?)",
        )
        .bind(id as i64)
        .bind(rmp_serde::to_vec(&ip)?)
        .bind(rmp_serde::to_vec(&status)?)
        .bind(timestamp_int as i64)
        .execute(&self.connection)
        .await?;
        Ok(())
    }
    pub async fn get_account_storage(&self, user_id: u32) -> Result<AccountStorages, Error> {
        let row = sqlx::query("select Data from Users where Id = ?")
            .bind(user_id as i64)
            .fetch_one(&self.connection)
            .await?;
        let user_data: UserData = rmp_serde::from_slice(row.try_get("Data")?)?;
        Ok(user_data.storage)
    }
    pub async fn put_account_storage(
        &self,
        user_id: u32,
        storage: AccountStorages,
    ) -> Result<(), Error> {
        self.update_userdata(user_id, |user_data| user_data.storage = storage)
            .await
    }
    pub async fn get_settings(&self, id: u32) -> Result<AsciiString, Error> {
        let row = sqlx::query("select Data from Users where Id = ?")
            .bind(id as i64)
            .fetch_one(&self.connection)
            .await?;
        let user_data: UserData = rmp_serde::from_slice(row.try_get("Data")?)?;
        Ok(user_data.settings.into())
    }
    pub async fn save_settings(&self, id: u32, settings: &str) -> Result<(), Error> {
        self.update_userdata(id, |user_data| user_data.settings = settings.into())
            .await
    }
    pub async fn put_uuid(&self, user_id: u32, uuid: u64) -> Result<(), Error> {
        self.update_userdata(user_id, |user_data| user_data.last_uuid = uuid)
            .await
    }

    pub async fn get_ship_data(&self, psk: &[u8]) -> Result<bool, Error> {
        let count = sqlx::query("select count(*) from Ships where PSK = ?")
            .bind(psk)
            .fetch_one(&self.connection)
            .await?
            .try_get::<i64, _>(0)?;
        Ok(count != 0)
    }
    pub fn registration_enabled(&self) -> bool {
        self.registration_enabled
    }
    pub async fn put_ship_data(&self, psk: &[u8]) -> Result<(), Error> {
        sqlx::query("insert into Ships (PSK) values (?)")
            .bind(psk)
            .execute(&self.connection)
            .await?;
        Ok(())
    }

    async fn update_userdata<F>(&self, user_id: u32, f: F) -> Result<(), Error>
    where
        F: FnOnce(&mut UserData) + Send,
    {
        let mut transaction = self.connection.begin().await?;
        let row = sqlx::query("select Data from Users where Id = ?")
            .bind(user_id as i64)
            .fetch_one(&mut *transaction)
            .await?;
        let mut user_data: UserData = rmp_serde::from_slice(row.try_get("Data")?)?;
        f(&mut user_data);
        sqlx::query("update Users set Data = ? where Id = ?")
            .bind(rmp_serde::to_vec(&user_data)?)
            .bind(user_id as i64)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::sql::Sql;
    use data_structs::flags::Flags;
    use pso2packetlib::{
        protocol::{
            login::{LoginResult, UserInfoPacket},
            models::SGValue,
        },
        AsciiString,
    };
    use std::{net::Ipv4Addr, time::Duration};

    #[tokio::test]
    async fn test_master_db() {
        let _ = std::fs::remove_file("test.db");
        let db = Sql::new("sqlite:test.db", false)
            .await
            .expect("DB creation failed");

        let (segaid, pass) = ("username", "password");

        let mut created_user = db
            .create_sega_user(segaid, pass)
            .await
            .expect("SEGAID user creation failed");
        let login_user = db
            .get_sega_user(segaid, pass, Ipv4Addr::UNSPECIFIED)
            .await
            .expect("SEGAID user login failed");
        assert_eq!(created_user, login_user);

        let user_info = UserInfoPacket {
            free_sg: SGValue(10.0),
            premium_expiration: Duration::from_secs(10),
            ..Default::default()
        };
        db.put_user_info(created_user.id, user_info.clone())
            .await
            .expect("User info insertion failed");
        let read_user_info = db
            .get_user_info(created_user.id)
            .await
            .expect("Failed to get user info");
        assert_eq!(user_info, read_user_info);

        let mut flags = Flags::new();
        flags.set(10, 1);
        db.put_account_flags(created_user.id, flags.clone())
            .await
            .expect("Account flags insertion failed");
        created_user.account_flags = flags;
        db.put_uuid(created_user.id, 199)
            .await
            .expect("Failed to insert uuid");
        created_user.last_uuid = 199;

        let challenge = db
            .new_challenge(created_user.id)
            .await
            .expect("Challenge creation failed");
        let challenge_user = db
            .login_challenge(created_user.id, challenge)
            .await
            .expect("Challenge login failed");
        assert_eq!(created_user, challenge_user);
        db.drop_challenges()
            .await
            .expect("Dropping challenges failed");

        let psn_username = "psnusername";

        let psn_user = db
            .create_psn_user(psn_username)
            .await
            .expect("PSN User creation failed");
        let login_psn_user = db
            .get_psn_user(psn_username, Ipv4Addr::UNSPECIFIED)
            .await
            .expect("PSN User login failed");
        assert_eq!(psn_user, login_psn_user);

        let logins = db
            .get_logins(created_user.id)
            .await
            .expect("Login attempts request failed");
        assert!(!logins.is_empty());
        let login = &logins[0];
        assert_eq!(login.ip, Ipv4Addr::UNSPECIFIED);
        assert_eq!(login.status, LoginResult::Successful);

        let settings = AsciiString::from("a");
        db.save_settings(created_user.id, &settings)
            .await
            .expect("Failed to save settings");
        let read_settings = db
            .get_settings(created_user.id)
            .await
            .expect("Failed to read settings");
        assert_eq!(read_settings, settings);

        let _ = std::fs::remove_file("test.db");
    }
}
