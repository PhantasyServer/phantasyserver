use crate::{
    inventory::Inventory, master_conn::MasterConnection, mutex::Mutex, palette::Palette, Error,
};
use data_structs::{
    flags::Flags,
    inventory::AccountStorages,
    master_ship::{MasterShipAction, SetNicknameResult, UserCreds, UserLoginResult},
};
use pso2packetlib::{
    protocol::{
        login::{Language, LoginAttempt, UserInfoPacket},
        models::character::Character,
        PacketType,
    },
    AsciiString,
};
use sqlx::{migrate::MigrateDatabase, Executor, Row};
use std::net::Ipv4Addr;

pub struct Sql {
    connection: sqlx::SqlitePool,
    master_ship: Mutex<MasterConnection>,
}

#[derive(Default)]
pub struct User {
    pub id: u32,
    pub nickname: String,
    pub lang: Language,
    pub packet_type: PacketType,
    pub accountflags: Flags,
    pub isgm: bool,
    pub last_uuid: u64,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct UserData {
    character_ids: Vec<u32>,
    symbol_arts: Vec<u128>,
}

#[derive(Default, serde::Serialize, serde::Deserialize, Clone)]
#[serde(default)]
pub struct CharData {
    pub character: Character,
    pub inventory: Inventory,
    pub palette: Palette,
    pub flags: Flags,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct ChallengeData {
    pub lang: Language,
    pub packet_type: PacketType,
}

impl Sql {
    pub async fn new(path: &str, master_ship: Mutex<MasterConnection>) -> Result<Self, Error> {
        sqlx::any::install_default_drivers();
        let conn = if !sqlx::Sqlite::database_exists(path).await.unwrap_or(false) {
            Self::create_db(path).await?
        } else {
            let conn = sqlx::SqlitePool::connect(path).await?;
            sqlx::query("delete from Challenges").execute(&conn).await?;
            conn
        };
        Ok(Self {
            connection: conn,
            master_ship,
        })
    }

    async fn create_db(path: &str) -> Result<sqlx::SqlitePool, Error> {
        sqlx::Sqlite::create_database(path).await?;
        let conn = sqlx::SqlitePool::connect(path).await?;
        conn.execute(
            "
            create table if not exists Users (
                Id integer primary key autoincrement,
                Data blob
            );
        ",
        )
        .await?;
        conn.execute(
            "
            create table if not exists Characters (
                Id integer primary key autoincrement,
                Data blob
            );
        ",
        )
        .await?;
        conn.execute(
            "
            create table if not exists SymbolArts (
                UUID blob,
                Name blob,
                Data blob
            );
        ",
        )
        .await?;
        conn.execute(
            "
            create table if not exists Challenges (
                Challenge integer,
                Data blob
            );
        ",
        )
        .await?;
        Ok(conn)
    }

    pub async fn run_action(&self, action: MasterShipAction) -> Result<MasterShipAction, Error> {
        MasterConnection::run_action(&self.master_ship, action).await
    }

    pub async fn get_sega_user(
        &self,
        username: &str,
        password: &str,
        ip: Ipv4Addr,
    ) -> Result<User, Error> {
        let result = self
            .run_action(MasterShipAction::UserLogin(UserCreds {
                username: username.to_string(),
                password: password.to_string(),
                ip,
            }))
            .await?;
        match result {
            MasterShipAction::UserLoginResult(UserLoginResult::Success {
                id,
                nickname,
                accountflags,
                isgm,
                last_uuid,
            }) => {
                if sqlx::query("select count(*) from Users where Id = ?")
                    .bind(id as i64)
                    .fetch_one(&self.connection)
                    .await?
                    .get::<i64, _>(0)
                    == 0
                {
                    self.insert_local_user(id).await?;
                }
                Ok(User {
                    id,
                    nickname,
                    accountflags,
                    isgm,
                    last_uuid,
                    ..Default::default()
                })
            }
            MasterShipAction::UserLoginResult(UserLoginResult::InvalidPassword(_)) => {
                Err(Error::InvalidPassword)
            }
            MasterShipAction::UserLoginResult(UserLoginResult::NotFound) => {
                self.create_sega_user(username, password).await
            }
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn get_psn_user(&self, username: &str, ip: Ipv4Addr) -> Result<User, Error> {
        let result = self
            .run_action(MasterShipAction::UserLoginVita(UserCreds {
                username: username.to_string(),
                password: String::new(),
                ip,
            }))
            .await?;
        match result {
            MasterShipAction::UserLoginResult(UserLoginResult::Success {
                id,
                nickname,
                accountflags,
                isgm,
                last_uuid,
            }) => {
                if sqlx::query("select count(*) from Users where Id = ?")
                    .bind(id as i64)
                    .fetch_one(&self.connection)
                    .await?
                    .get::<i64, _>(0)
                    == 0
                {
                    self.insert_local_user(id).await?;
                }
                Ok(User {
                    id,
                    nickname,
                    accountflags,
                    isgm,
                    last_uuid,
                    ..Default::default()
                })
            }
            MasterShipAction::UserLoginResult(UserLoginResult::InvalidPassword(_)) => {
                Err(Error::MSUnexpected)
            }
            MasterShipAction::UserLoginResult(UserLoginResult::NotFound) => {
                self.create_psn_user(username).await
            }
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    async fn create_sega_user(&self, username: &str, password: &str) -> Result<User, Error> {
        let result = self
            .run_action(MasterShipAction::UserRegister(UserCreds {
                username: username.to_string(),
                password: password.to_string(),
                ip: Ipv4Addr::UNSPECIFIED,
            }))
            .await?;
        let user = match result {
            MasterShipAction::UserLoginResult(UserLoginResult::Success {
                id,
                nickname,
                accountflags,
                isgm,
                last_uuid,
            }) => Ok(User {
                id,
                nickname,
                accountflags,
                isgm,
                last_uuid,
                ..Default::default()
            }),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }?;
        self.insert_local_user(user.id).await?;
        Ok(user)
    }
    async fn create_psn_user(&self, username: &str) -> Result<User, Error> {
        let result = self
            .run_action(MasterShipAction::UserRegisterVita(UserCreds {
                username: username.to_string(),
                password: String::new(),
                ip: Ipv4Addr::UNSPECIFIED,
            }))
            .await?;
        let user = match result {
            MasterShipAction::UserLoginResult(UserLoginResult::Success {
                id,
                nickname,
                accountflags,
                isgm,
                last_uuid,
            }) => Ok(User {
                id,
                nickname,
                accountflags,
                isgm,
                last_uuid,
                ..Default::default()
            }),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }?;
        self.insert_local_user(user.id).await?;
        Ok(user)
    }

    async fn insert_local_user(&self, user_id: u32) -> Result<(), Error> {
        let user_data = UserData {
            symbol_arts: vec![0; 10],
            ..Default::default()
        };
        sqlx::query("insert into Users (Id, Data) values (?,?)")
            .bind(user_id as i64)
            .bind(rmp_serde::to_vec(&user_data)?)
            .execute(&self.connection)
            .await?;
        Ok(())
    }

    pub async fn get_user_info(&self, user_id: u32) -> Result<UserInfoPacket, Error> {
        let result = self
            .run_action(MasterShipAction::GetUserInfo(user_id))
            .await?;
        match result {
            MasterShipAction::UserInfo(info) => Ok(info),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn put_user_info(&self, user_id: u32, info: UserInfoPacket) -> Result<(), Error> {
        let result = self
            .run_action(MasterShipAction::PutUserInfo { id: user_id, info })
            .await?;
        match result {
            MasterShipAction::Ok => Ok(()),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn put_account_flags(&self, user_id: u32, flags: Flags) -> Result<(), Error> {
        let result = self
            .run_action(MasterShipAction::PutAccountFlags { id: user_id, flags })
            .await?;
        match result {
            MasterShipAction::Ok => Ok(()),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn new_challenge(
        &self,
        user_id: u32,
        challenge_data: ChallengeData,
    ) -> Result<u32, Error> {
        let result = self
            .run_action(MasterShipAction::NewBlockChallenge(user_id))
            .await?;
        match result {
            MasterShipAction::BlockChallengeResult(challenge) => {
                sqlx::query("insert into Challenges (Challenge, Data) values (?,?)")
                    .bind(challenge as i64)
                    .bind(rmp_serde::to_vec(&challenge_data)?)
                    .execute(&self.connection)
                    .await?;
                Ok(challenge)
            }
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn login_challenge(&self, user_id: u32, challenge: u32) -> Result<User, Error> {
        let result = self
            .run_action(MasterShipAction::ChallengeLogin {
                challenge,
                player_id: user_id,
            })
            .await?;
        match result {
            MasterShipAction::UserLoginResult(UserLoginResult::Success {
                id,
                nickname,
                accountflags,
                isgm,
                last_uuid,
            }) => {
                let row = sqlx::query("select * from Challenges where Challenge = ?")
                    .bind(challenge as i64)
                    .fetch_one(&self.connection)
                    .await?;
                let challenge_data: ChallengeData = rmp_serde::from_slice(row.try_get("Data")?)?;
                Ok(User {
                    id,
                    nickname,
                    lang: challenge_data.lang,
                    packet_type: challenge_data.packet_type,
                    accountflags,
                    isgm,
                    last_uuid,
                })
            }
            MasterShipAction::UserLoginResult(UserLoginResult::InvalidPassword(_)) => {
                Err(Error::MSUnexpected)
            }
            MasterShipAction::UserLoginResult(UserLoginResult::NotFound) => Err(Error::NoUser),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn get_logins(&self, id: u32) -> Result<Vec<LoginAttempt>, Error> {
        let result = self.run_action(MasterShipAction::GetLogins(id)).await?;
        match result {
            MasterShipAction::GetLoginsResult(d) => Ok(d),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn get_settings(&self, id: u32) -> Result<AsciiString, Error> {
        let result = self.run_action(MasterShipAction::GetSettings(id)).await?;
        match result {
            MasterShipAction::GetSettingsResult(d) => Ok(d),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn save_settings(&self, id: u32, settings: &str) -> Result<(), Error> {
        let result = self
            .run_action(MasterShipAction::PutSettings {
                id,
                settings: settings.into(),
            })
            .await?;
        match result {
            MasterShipAction::Ok => Ok(()),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn get_characters(&self, id: u32) -> Result<Vec<CharData>, Error> {
        let mut chars = vec![];
        let row = sqlx::query("select Data from Users where Id = ?")
            .bind(id as i64)
            .fetch_one(&self.connection)
            .await?;
        let user_data: UserData = rmp_serde::from_slice(row.try_get("Data")?)?;
        for char_id in user_data.character_ids {
            let row = sqlx::query("select Data from Characters where Id = ?")
                .bind(char_id as i64)
                .fetch_optional(&self.connection)
                .await?;
            if let Some(data) = row {
                let mut char: CharData = rmp_serde::from_slice(data.try_get("Data")?)?;
                char.character.player_id = id;
                char.character.character_id = char_id;
                chars.push(char);
            }
        }
        Ok(chars)
    }
    pub async fn get_character(&self, id: u32, char_id: u32) -> Result<CharData, Error> {
        let row = sqlx::query("select Data from Characters where Id = ?")
            .bind(char_id as i64)
            .fetch_one(&self.connection)
            .await?;
        let mut char: CharData = rmp_serde::from_slice(row.try_get("Data")?)?;
        char.character.player_id = id;
        char.character.character_id = char_id;
        char.inventory.storages = self.get_account_storage(id).await?;
        Ok(char)
    }
    pub async fn update_character(&self, char: &CharData) -> Result<(), Error> {
        sqlx::query("update Characters set Data = ? where Id = ?")
            .bind(rmp_serde::to_vec(&char)?)
            .bind(char.character.character_id as i64)
            .execute(&self.connection)
            .await?;
        Ok(())
    }
    pub async fn put_character(&self, id: u32, char: CharData) -> Result<u32, Error> {
        let mut transaction = self.connection.begin().await?;
        let data = rmp_serde::to_vec(&char)?;
        let char_id = sqlx::query("insert into Characters (Data) values (?) returning Id")
            .bind(&data)
            .fetch_one(&mut *transaction)
            .await?
            .try_get::<i64, _>("Id")?;
        transaction.commit().await?;

        self.update_userdata(id, |user_data| user_data.character_ids.push(char_id as u32))
            .await?;

        Ok(char_id as u32)
    }
    pub async fn delete_character(&self, id: u32, char_id: u32) -> Result<(), Error> {
        let mut transaction = self.connection.begin().await?;
        sqlx::query("delete from Characters where Id = ?")
            .bind(char_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        self.update_userdata(id, |user_data| {
            if let Some((pos, _)) = user_data
                .character_ids
                .iter()
                .enumerate()
                .find(|(_, &i)| i == char_id)
            {
                user_data.character_ids.swap_remove(pos);
            }
        })
        .await?;

        Ok(())
    }
    pub async fn get_symbol_art_list(&self, id: u32) -> Result<Vec<u128>, Error> {
        let row = sqlx::query("select Data from Users where Id = ?")
            .bind(id as i64)
            .fetch_one(&self.connection)
            .await?;
        let user_data: UserData = rmp_serde::from_slice(row.try_get("Data")?)?;
        Ok(user_data.symbol_arts)
    }
    pub async fn set_symbol_art_list(&self, uuids: Vec<u128>, id: u32) -> Result<(), Error> {
        self.update_userdata(id, |user_data| user_data.symbol_arts = uuids)
            .await
    }
    pub async fn get_symbol_art(&self, uuid: u128) -> Result<Option<Vec<u8>>, Error> {
        let row = sqlx::query("select * from SymbolArts where UUID = ?")
            .bind(format!("{uuid:X}").as_bytes())
            .fetch_optional(&self.connection)
            .await?;
        match row {
            Some(data) => Ok(Some(data.try_get::<Vec<u8>, _>("Data")?)),
            None => Ok(None),
        }
    }
    pub async fn add_symbol_art(&self, uuid: u128, data: &[u8], name: &str) -> Result<(), Error> {
        sqlx::query("insert into SymbolArts (UUID, Name, Data) values (?, ?, ?)")
            .bind(format!("{uuid:X}").as_bytes())
            .bind(name.as_bytes())
            .bind(data)
            .execute(&self.connection)
            .await?;
        Ok(())
    }
    pub async fn get_account_storage(&self, user_id: u32) -> Result<AccountStorages, Error> {
        let result = self
            .run_action(MasterShipAction::GetStorage(user_id))
            .await?;
        match result {
            MasterShipAction::GetStorageResult(storages) => Ok(storages),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn update_account_storage(&self, user_id: u32, inv: &Inventory) -> Result<(), Error> {
        let result = self
            .run_action(MasterShipAction::PutStorage {
                id: user_id,
                storage: inv.storages.clone(),
            })
            .await?;
        match result {
            MasterShipAction::Ok => Ok(()),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn put_uuid(&self, user_id: u32, uuid: u64) -> Result<(), Error> {
        let result = self
            .run_action(MasterShipAction::PutUUID { id: user_id, uuid })
            .await?;
        match result {
            MasterShipAction::Ok => Ok(()),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn set_username(
        &self,
        user_id: u32,
        nickname: &str,
    ) -> Result<SetNicknameResult, Error> {
        let result = self
            .run_action(MasterShipAction::SetNickname {
                id: user_id,
                nickname: nickname.to_string(),
            })
            .await?;
        match result {
            MasterShipAction::SetNicknameResult(res) => Ok(res),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
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
