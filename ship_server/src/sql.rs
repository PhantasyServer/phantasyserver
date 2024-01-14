use crate::{inventory::Inventory, master_conn::MasterConnection, palette::Palette, Error};
use data_structs::{flags::Flags, AccountStorages, MasterShipAction, UserCreds};
use parking_lot::Mutex;
use pso2packetlib::{
    protocol::{
        login::{Language, LoginAttempt, UserInfoPacket},
        models::character::Character,
        PacketType,
    },
    AsciiString,
};
use sqlx::{migrate::MigrateDatabase, Connection, Executor, Row};
use std::net::Ipv4Addr;

pub struct Sql {
    connection: sqlx::AnyPool,
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
}

pub struct CharData {
    pub character: Character,
    pub flags: Flags,
}

impl Sql {
    pub async fn new(path: &str, master_ship: Mutex<MasterConnection>) -> Result<Self, Error> {
        sqlx::any::install_default_drivers();
        if !sqlx::Any::database_exists(path).await.unwrap_or(false) {
            return Self::create_db(path, master_ship).await;
        }
        let conn = sqlx::AnyPool::connect(path).await?;
        sqlx::query("delete from Challenges").execute(&conn).await?;
        Ok(Self {
            connection: conn,
            master_ship,
        })
    }

    async fn create_db(path: &str, master_ship: Mutex<MasterConnection>) -> Result<Self, Error> {
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
                CharacterIds blob default NULL,
                SymbolArtIds blob default NULL
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
            create table if not exists Characters (
                Id integer primary key {},
                Data blob default NULL,
                Inventory blob default NULL,
                Palette blob default NULL,
                Flags blob default NULL
            );
        ",
                auto_inc
            )
            .as_str(),
        )
        .await?;
        conn.execute(
            "
            create table if not exists SymbolArts (
                UUID blob default NULL,
                Name blob default NULL,
                Data blob default NULL
            );
        ",
        )
        .await?;
        conn.execute(
            "
            create table if not exists ServerStats (
                Tag blob default NULL,
                Value blob default NULL
            );
        ",
        )
        .await?;
        conn.execute(
            "
            create table if not exists Challenges (
                Challenge integer default 0,
                Lang blob default NULL,
                PacketType blob default NULL
            );
        ",
        )
        .await?;
        sqlx::query("insert into ServerStats (Tag, Value) values (?, ?)")
            .bind("UUID".as_bytes())
            .bind("1".as_bytes())
            .execute(&conn)
            .await?;
        Ok(Self {
            connection: conn,
            master_ship,
        })
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
            MasterShipAction::UserLoginResult(d) => match d {
                data_structs::UserLoginResult::Success {
                    id,
                    nickname,
                    accountflags,
                    isgm,
                } => {
                    if sqlx::query("select count(*) from Users where Id = ?")
                        .bind(id as i64)
                        .fetch_one(&self.connection)
                        .await?
                        .get::<i64, _>(0)
                        == 0
                    {
                        sqlx::query("insert into Users (Id) values (?)")
                            .bind(id as i64)
                            .execute(&self.connection)
                            .await?;
                    }
                    Ok(User {
                        id,
                        nickname,
                        accountflags,
                        isgm,
                        ..Default::default()
                    })
                }
                data_structs::UserLoginResult::InvalidPassword(_) => Err(Error::InvalidPassword),
                data_structs::UserLoginResult::NotFound => {
                    self.create_sega_user(username, password).await
                }
            },
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
            MasterShipAction::UserLoginResult(d) => match d {
                data_structs::UserLoginResult::Success {
                    id,
                    nickname,
                    accountflags,
                    isgm,
                } => {
                    if sqlx::query("select count(*) from Users where Id = ?")
                        .bind(id as i64)
                        .fetch_one(&self.connection)
                        .await?
                        .get::<i64, _>(0)
                        == 0
                    {
                        sqlx::query("insert into Users (Id) values (?)")
                            .bind(id as i64)
                            .execute(&self.connection)
                            .await?;
                    }
                    Ok(User {
                        id,
                        nickname,
                        accountflags,
                        isgm,
                        ..Default::default()
                    })
                }
                data_structs::UserLoginResult::InvalidPassword(_) => Err(Error::InvalidPassword),
                data_structs::UserLoginResult::NotFound => self.create_psn_user(username).await,
            },
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
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
            MasterShipAction::UserLoginResult(data_structs::UserLoginResult::Success {
                id,
                nickname,
                accountflags,
                isgm,
            }) => Ok(User {
                id,
                nickname,
                accountflags,
                isgm,
                ..Default::default()
            }),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }?;
        sqlx::query("insert into Users (Id) values (?)")
            .bind(user.id as i64)
            .execute(&self.connection)
            .await?;
        Ok(user)
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
            MasterShipAction::UserLoginResult(data_structs::UserLoginResult::Success {
                id,
                nickname,
                accountflags,
                isgm,
            }) => Ok(User {
                id,
                nickname,
                accountflags,
                isgm,
                ..Default::default()
            }),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }?;
        sqlx::query("insert into Users (Id) values (?)")
            .bind(user.id as i64)
            .execute(&self.connection)
            .await?;

        Ok(user)
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
        lang: Language,
        packet: PacketType,
    ) -> Result<u32, Error> {
        let result = self
            .run_action(MasterShipAction::NewBlockChallenge(user_id))
            .await?;
        match result {
            MasterShipAction::BlockChallengeResult(challenge) => {
                sqlx::query("insert into Challenges (Challenge, Lang, PacketType) values (?,?,?)")
                    .bind(challenge as i64)
                    .bind(serde_json::to_vec(&lang)?)
                    .bind(serde_json::to_vec(&packet)?)
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
            MasterShipAction::UserLoginResult(d) => match d {
                data_structs::UserLoginResult::Success {
                    id,
                    nickname,
                    accountflags,
                    isgm,
                } => {
                    let row = sqlx::query("select * from Challenges where Challenge = ?")
                        .bind(challenge as i64)
                        .fetch_one(&self.connection)
                        .await?;
                    let lang = serde_json::from_slice(row.try_get("Lang")?)?;
                    let packet_type = serde_json::from_slice(row.try_get("PacketType")?)?;
                    Ok(User {
                        id,
                        nickname,
                        lang,
                        packet_type,
                        accountflags,
                        isgm,
                    })
                }
                data_structs::UserLoginResult::InvalidPassword(_) => Err(Error::MSUnexpected),
                data_structs::UserLoginResult::NotFound => Err(Error::NoUser),
            },
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
    pub async fn get_characters(&self, id: u32) -> Result<Vec<Character>, Error> {
        let mut chars = vec![];
        let row = sqlx::query("select CharacterIds from Users where Id = ?")
            .bind(id as i64)
            .fetch_one(&self.connection)
            .await?;
        let ids = match row.try_get("CharacterIds") {
            Ok(d) => serde_json::from_slice::<Vec<i64>>(d)?,
            Err(_) => Default::default(),
        };
        for char_id in ids {
            let row = sqlx::query("select Data from Characters where Id = ?")
                .bind(char_id)
                .fetch_optional(&self.connection)
                .await?;
            if let Some(data) = row {
                let mut char: Character = serde_json::from_slice(data.try_get("Data")?)?;
                char.player_id = id;
                char.character_id = char_id as u32;
                chars.push(char);
            }
        }
        Ok(chars)
    }
    pub async fn get_character(&self, id: u32, char_id: u32) -> Result<CharData, Error> {
        let row = sqlx::query("select * from Characters where Id = ?")
            .bind(char_id as i64)
            .fetch_one(&self.connection)
            .await?;
        let mut char: Character = serde_json::from_slice(row.try_get("Data")?)?;
        char.player_id = id;
        char.character_id = char_id;
        let flags: Flags = match row.try_get("Flags") {
            Ok(d) => rmp_serde::from_slice(d)?,
            Err(_) => Default::default(),
        };
        Ok(CharData {
            character: char,
            flags,
        })
    }
    pub async fn update_character(&self, char: &Character) -> Result<(), Error> {
        sqlx::query("update Characters set Data = ? where Id = ?")
            .bind(serde_json::to_string(&char)?.as_bytes())
            .bind(char.character_id as i64)
            .execute(&self.connection)
            .await?;
        Ok(())
    }
    pub async fn update_char_flags(&self, char_id: u32, flags: Flags) -> Result<(), Error> {
        sqlx::query("update Characters set Flags = ? where Id = ?")
            .bind(rmp_serde::to_vec(&flags)?)
            .bind(char_id as i64)
            .execute(&self.connection)
            .await?;
        Ok(())
    }
    pub async fn put_character(&self, id: u32, char: &Character) -> Result<u32, Error> {
        let row = sqlx::query("select CharacterIds from Users where Id = ?")
            .bind(id as i64)
            .fetch_one(&self.connection)
            .await?;
        let mut ids = match row.try_get("CharacterIds") {
            Ok(d) => serde_json::from_slice::<Vec<i64>>(d)?,
            Err(_) => Default::default(),
        };
        let data = serde_json::to_string(&char)?;
        sqlx::query("insert into Characters (Data) values (?)")
            .bind(data.as_bytes())
            .execute(&self.connection)
            .await?;
        let char_id = sqlx::query("select Id from Characters where Data = ?")
            .bind(data.as_bytes())
            .fetch_one(&self.connection)
            .await?
            .try_get::<i64, _>("Id")?;
        ids.push(char_id);
        sqlx::query("update Users set CharacterIds = ? where Id = ?")
            .bind(serde_json::to_string(&ids)?.as_bytes())
            .bind(id as i64)
            .execute(&self.connection)
            .await?;
        Ok(char_id as u32)
    }
    pub async fn get_symbol_art_list(&self, id: u32) -> Result<Vec<u128>, Error> {
        let ids = sqlx::query("select SymbolArtIds from Users where Id = ?")
            .bind(id as i64)
            .fetch_one(&self.connection)
            .await?;
        match ids.try_get("SymbolArtIds") {
            Ok(data) => Ok(serde_json::from_slice(data)?),
            Err(_) => Ok(vec![0; 20]),
        }
    }
    pub async fn set_symbol_art_list(&self, uuids: Vec<u128>, id: u32) -> Result<(), Error> {
        sqlx::query("update Users set SymbolArtIds = ? where Id = ?")
            .bind(serde_json::to_string(&uuids)?.as_bytes())
            .bind(id as i64)
            .execute(&self.connection)
            .await?;
        Ok(())
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
    pub async fn get_inventory(&self, char_id: u32, user_id: u32) -> Result<Inventory, Error> {
        let mut inventory = self.get_player_inventory(char_id).await?;
        inventory.storages = self.get_account_storage(user_id).await?;
        Ok(inventory)
    }
    async fn get_player_inventory(&self, char_id: u32) -> Result<Inventory, Error> {
        let row = sqlx::query("select Inventory from Characters where Id = ?")
            .bind(char_id as i64)
            .fetch_one(&self.connection)
            .await?;
        match row.try_get("Inventory") {
            Ok(d) => Ok(serde_json::from_slice(d)?),
            Err(_) => Ok(Default::default()),
        }
    }
    async fn get_account_storage(&self, user_id: u32) -> Result<AccountStorages, Error> {
        let result = self
            .run_action(MasterShipAction::GetStorage(user_id))
            .await?;
        match result {
            MasterShipAction::GetStorageResult(storages) => Ok(storages),
            MasterShipAction::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn update_inventory(
        &self,
        char_id: u32,
        user_id: u32,
        inv: &Inventory,
    ) -> Result<(), Error> {
        sqlx::query("update Characters set Inventory = ? where Id = ?")
            .bind(serde_json::to_string(&inv)?.as_bytes())
            .bind(char_id as i64)
            .execute(&self.connection)
            .await?;
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
    pub async fn get_uuid(&self) -> Result<u64, Error> {
        Ok(sqlx::query("select Value from ServerStats where Tag = ?")
            .bind("UUID".as_bytes())
            .fetch_one(&self.connection)
            .await?
            .try_get::<i64, _>("UUID")? as u64)
    }
    pub async fn set_uuid(&self, uuid: u64) -> Result<(), Error> {
        sqlx::query("update ServerStats set Value = ? where Tag = ?")
            .bind(uuid as i64)
            .bind("UUID".as_bytes())
            .execute(&self.connection)
            .await?;
        Ok(())
    }
    pub async fn get_palette(&self, char_id: u32) -> Result<Palette, Error> {
        let row = sqlx::query("select Palette from Characters where Id = ?")
            .bind(char_id as i64)
            .fetch_one(&self.connection)
            .await?;
        match row.try_get("Palette") {
            Ok(d) => Ok(serde_json::from_slice(d)?),
            Err(_) => Ok(Default::default()),
        }
    }
    pub async fn update_palette(&self, char_id: u32, palette: &Palette) -> Result<(), Error> {
        sqlx::query("update Characters set Palette = ? where Id = ?")
            .bind(serde_json::to_string(palette)?.as_bytes())
            .bind(char_id as i64)
            .execute(&self.connection)
            .await?;
        Ok(())
    }
}
