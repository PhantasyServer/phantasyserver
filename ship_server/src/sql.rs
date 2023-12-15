use std::{
    fs::File,
    net::Ipv4Addr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    inventory::{AccountStorages, Inventory},
    palette::Palette,
    Error,
};
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher};
use pso2packetlib::{
    protocol::{
        login::{LoginAttempt, LoginResult},
        models::character::Character,
    },
    AsciiString,
};
use rsa::rand_core::OsRng;
use sqlite::{ConnectionWithFullMutex as SqliteConnection, State, Type, Value};

pub struct Sql {
    connection: SqliteConnection,
}

pub struct User {
    pub id: u32,
    pub nickname: String,
}

impl Sql {
    pub fn new() -> sqlite::Result<Sql> {
        let connection = sqlite::Connection::open_with_full_mutex("server.db")?;
        let query = "
            create table if not exists Users (
                Id integer primary key autoincrement,
                Username text default NULL,
                Nickname text default NULL,
                Password text default NULL,
                PSNNickname text default NULL,
                Settings text default NULL,
                CharacterIds text default NULL,
                SymbolArtIds text default NULL,
                Storage text default NULL
            );
        ";
        connection.execute(query)?;
        let query = "
            create table if not exists Characters (
                Id integer primary key autoincrement,
                Data text default NULL,
                Inventory text default NULL,
                Palette text default NULL
            );
        ";
        connection.execute(query)?;
        let query = "
            create table if not exists Logins (
                Id integer primary key autoincrement,
                UserId integer default NULL,
                IpAddress text default NULL,
                Status text default NULL,
                Timestamp integer default NULL
            );
        ";
        connection.execute(query)?;
        let query = "
            create table if not exists SymbolArts (
                UUID string default NULL,
                name string default NULL,
                data blob default NULL
            );
        ";
        connection.execute(query)?;
        let query = "
            create table if not exists ServerStats (
                Tag string default NULL,
                Value string default NULL
            );
        ";
        connection.execute(query)?;
        Ok(Sql { connection })
    }

    pub fn get_sega_user(&mut self, username: &str, password: &str) -> Result<User, Error> {
        if username.is_empty() || password.is_empty() {
            return Err(Error::InvalidInput);
        }
        let query = "select * from Users where Username = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, username))?;
        match statement.next()? {
            State::Row => {
                let stored_password = statement.read::<String, _>("Password")?;
                let id = statement.read::<i64, _>("Id")? as u32;
                let col_typee = statement.column_type("Nickname")?;
                let nickname = if let Type::Null = col_typee {
                    String::new()
                } else {
                    statement.read::<String, _>("Nickname")?
                };
                let hash = match PasswordHash::new(&stored_password) {
                    Ok(x) => x,
                    Err(_) => return Err(Error::InvalidPassword(id)),
                };
                match hash.verify_password(&[&Argon2::default()], password) {
                    Ok(_) => {}
                    Err(_) => return Err(Error::InvalidPassword(id)),
                }
                Ok(User { id, nickname })
            }
            State::Done => {
                drop(statement);
                self.create_sega_user(username, password)
            }
        }
    }
    pub fn get_psn_user(&mut self, username: &str) -> Result<User, Error> {
        if username.is_empty() {
            return Err(Error::InvalidInput);
        }
        let query = "select * from Users where PSNNickname = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, username))?;
        match statement.next()? {
            State::Row => {
                let id = statement.read::<i64, _>("Id")? as u32;
                let col_typee = statement.column_type("Nickname")?;
                let nickname = if let Type::Null = col_typee {
                    String::new()
                } else {
                    statement.read::<String, _>("Nickname")?
                };
                Ok(User { id, nickname })
            }
            State::Done => {
                drop(statement);
                self.create_psn_user(username)
            }
        }
    }
    fn create_psn_user(&mut self, username: &str) -> Result<User, Error> {
        let query = "insert into Users (PSNNickname, Settings) values (?, ?)";
        let settings_file = File::open("settings.txt")?;
        let settings = std::io::read_to_string(settings_file)?;
        let mut statement = self.connection.prepare(query)?;
        statement.bind(&[(1, username), (2, settings.as_str())][..])?;
        statement.into_iter().count();
        let query = "select Id from Users where PSNNickname = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, username))?;
        if let State::Row = statement.next()? {
            let id = statement.read::<i64, _>("Id")? as u32;
            Ok(User {
                id,
                nickname: String::new(),
            })
        } else {
            Err(Error::HashError)
        }
    }
    fn create_sega_user(&mut self, username: &str, password: &str) -> Result<User, Error> {
        let query = "insert into Users (Username, Password, Settings) values (?, ?, ?)";
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let hash = match argon2.hash_password(password.as_bytes(), &salt) {
            Ok(x) => x.to_string(),
            Err(_) => return Err(Error::HashError),
        };
        let settings_file = File::open("settings.txt")?;
        let settings = std::io::read_to_string(settings_file)?;
        let mut statement = self.connection.prepare(query)?;
        statement.bind(&[(1, username), (2, &hash), (3, &settings)][..])?;
        statement.into_iter().count();
        let query = "select Id from Users where Username = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, username))?;
        if let State::Row = statement.next()? {
            let id = statement.read::<i64, _>("Id")? as u32;
            Ok(User {
                id,
                nickname: String::new(),
            })
        } else {
            Err(Error::HashError)
        }
    }
    pub fn get_logins(&self, id: u32) -> Result<Vec<LoginAttempt>, Error> {
        let mut attempts = vec![];
        let query = "select * from Logins where UserId = ? order by Timestamp desc limit 50";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, id as i64))?;
        while let State::Row = statement.next()? {
            let ip_data = statement.read::<String, _>("IpAddress")?;
            let status_data = statement.read::<String, _>("Status")?;
            let timestamp_int = statement.read::<i64, _>("Timestamp")?;
            let mut attempt = LoginAttempt::default();
            attempt.ip = serde_json::from_str(&ip_data)?;
            attempt.status = serde_json::from_str(&status_data)?;
            attempt.timestamp = Duration::from_secs(timestamp_int as u64);
            attempts.push(attempt);
        }
        Ok(attempts)
    }
    pub fn put_login(&mut self, id: u32, ip: Ipv4Addr, status: LoginResult) -> Result<(), Error> {
        let ip_data = serde_json::to_string(&ip)?;
        let status_data = serde_json::to_string(&status)?;
        let timestamp_int = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let query = "insert into Logins (UserId, IpAddress, Status, Timestamp) values (?, ?, ?, ?)";
        let mut statement = self.connection.prepare(query)?;
        statement.bind::<&[(_, Value)]>(
            &[
                (1, (id as i64).into()),
                (2, ip_data.into()),
                (3, status_data.into()),
                (4, (timestamp_int as i64).into()),
            ][..],
        )?;
        statement.into_iter().count();
        Ok(())
    }
    pub fn get_settings(&self, id: u32) -> Result<AsciiString, Error> {
        let query = "select Settings from Users where Id = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, id as i64))?;
        match statement.next()? {
            State::Row => {
                let settings = statement.read::<String, _>("Settings")?;
                Ok(settings.into())
            }
            State::Done => Ok(Default::default()),
        }
    }
    pub fn save_settings(&mut self, id: u32, settings: &str) -> Result<(), Error> {
        let query = "update Users set Settings = ? where Id = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind::<&[(_, Value)]>(&[(1, settings.into()), (2, (id as i64).into())][..])?;
        statement.into_iter().count();
        Ok(())
    }
    pub fn get_characters(&self, id: u32) -> Result<Vec<Character>, Error> {
        let mut chars = vec![];
        let query = "select CharacterIds from Users where id = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, id as i64))?;
        if let State::Row = statement.next()? {
            let col_typee = statement.column_type("CharacterIds")?;
            if let Type::Null = col_typee {
                return Ok(chars);
            }
            let ids = statement.read::<String, _>("CharacterIds")?;
            let ids = serde_json::from_str::<Vec<i64>>(&ids)?;
            for char_id in ids {
                let query = "select Data from Characters where Id = ?";
                let mut statement = self.connection.prepare(query)?;
                statement.bind((1, char_id))?;
                while let State::Row = statement.next()? {
                    let data = statement.read::<String, _>("Data")?;
                    let mut char: Character = serde_json::from_str(&data)?;
                    char.player_id = id;
                    char.character_id = char_id as u32;
                    chars.push(char);
                }
            }
        }
        Ok(chars)
    }
    pub fn get_character(&self, id: u32, char_id: u32) -> Result<Character, Error> {
        let query = "select Data from Characters where Id = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, char_id as i64))?;
        if let State::Row = statement.next()? {
            let data = statement.read::<String, _>("Data")?;
            let mut char: Character = serde_json::from_str(&data)?;
            char.player_id = id;
            char.character_id = char_id;
            return Ok(char);
        }
        Err(Error::InvalidCharacter)
    }
    pub fn update_character(&mut self, char: &Character) -> Result<(), Error> {
        let char_data = serde_json::to_string(&char)?;
        let char_id = char.character_id;
        let query = "update Characters set Data = ? where Id = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind::<&[(_, Value)]>(
            &[(1, char_data.as_str().into()), (2, (char_id as i64).into())][..],
        )?;
        statement.into_iter().count();
        Ok(())
    }
    pub fn put_character(&mut self, id: u32, char: &Character) -> Result<u32, Error> {
        let mut char_id = 0;
        let query = "select CharacterIds from Users where id = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, id as i64))?;
        if let State::Row = statement.next()? {
            let col_typee = statement.column_type("CharacterIds")?;
            let mut ids = if let Type::Null = col_typee {
                vec![]
            } else {
                let ids = statement.read::<String, _>("CharacterIds")?;
                serde_json::from_str::<Vec<i64>>(&ids)?
            };
            let data = serde_json::to_string(&char)?;
            let query = "insert into Characters (Data) values (?)";
            let mut statement = self.connection.prepare(query)?;
            statement.bind((1, data.as_str()))?;
            statement.into_iter().count();
            let query = "select last_insert_rowid()";
            let mut statement = self.connection.prepare(query)?;
            statement.next()?;
            let inserted_id = statement.read::<i64, _>(0)?;
            char_id = inserted_id as u32;
            ids.push(inserted_id);
            let ids = serde_json::to_string(&ids)?;
            let query = "update Users set CharacterIds = ? where Id = ?";
            let mut statement = self.connection.prepare(query)?;
            statement.bind::<&[(_, Value)]>(&[(1, ids.into()), (2, (id as i64).into())][..])?;
            statement.into_iter().count();
        }

        Ok(char_id)
    }
    pub fn get_symbol_art_list(&self, id: u32) -> Result<Vec<u128>, Error> {
        let mut ids = vec![0; 20];
        let query = "select SymbolArtIds from Users where id = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, id as i64))?;
        if let State::Row = statement.next()? {
            let col_typee = statement.column_type("SymbolArtIds")?;
            if let Type::Null = col_typee {
                let ids_str = serde_json::to_string(&ids)?;
                let query = "update Users set SymbolArtIds = ? where Id = ?";
                let mut statement = self.connection.prepare(query)?;
                statement
                    .bind::<&[(_, Value)]>(&[(1, ids_str.into()), (2, (id as i64).into())][..])?;
                statement.into_iter().count();
                return Ok(ids);
            }
            let ids_str = statement.read::<String, _>("SymbolArtIds")?;
            ids = serde_json::from_str::<Vec<u128>>(&ids_str)?;
        }
        Ok(ids)
    }
    pub fn set_symbol_art_list(&mut self, uuids: Vec<u128>, id: u32) -> Result<(), Error> {
        let uuids = serde_json::to_string(&uuids)?;
        let query = "update Users set SymbolArtIds = ? where Id = ?";
        let mut statement = self.connection.prepare(query)?;
        statement
            .bind::<&[(_, Value)]>(&[(1, uuids.as_str().into()), (2, (id as i64).into())][..])?;
        statement.into_iter().count();
        Ok(())
    }
    pub fn get_symbol_art(&self, uuid: u128) -> Result<Option<Vec<u8>>, Error> {
        let query = "select * from SymbolArts where UUID = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, format!("{uuid:X}").as_str()))?;
        if let State::Row = statement.next()? {
            let col_typee = statement.column_type("data")?;
            if let Type::Null = col_typee {
                return Ok(None);
            }
            let data = statement.read::<Vec<u8>, _>("data")?;
            return Ok(Some(data));
        }
        Ok(None)
    }
    pub fn add_symbol_art(&mut self, uuid: u128, data: &[u8], name: &str) -> Result<(), Error> {
        let query = "insert into SymbolArts (UUID, name, data) values (?, ?, ?)";
        let mut statement = self.connection.prepare(query)?;
        statement.bind::<&[(_, Value)]>(
            &[
                (1, format!("{uuid:X}").as_str().into()),
                (2, name.into()),
                (3, data.into()),
            ][..],
        )?;
        statement.into_iter().count();
        Ok(())
    }
    pub fn get_inventory(&self, char_id: u32, user_id: u32) -> Result<Inventory, Error> {
        let mut inventory = self.get_player_inventory(char_id)?;
        inventory.storages = self.get_account_storage(user_id)?;
        Ok(inventory)
    }
    fn get_player_inventory(&self, char_id: u32) -> Result<Inventory, Error> {
        let query = "select Inventory from Characters where Id = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, char_id as i64))?;
        if let State::Row = statement.next()? {
            let col_typee = statement.column_type("Inventory")?;
            if let Type::Null = col_typee {
                return Ok(Default::default());
            }
            let inventory = statement.read::<String, _>("Inventory")?;
            let storage = serde_json::from_str::<Inventory>(&inventory)?;
            return Ok(storage);
        }
        Ok(Default::default())
    }
    fn get_account_storage(&self, user_id: u32) -> Result<AccountStorages, Error> {
        let query = "select Storage from Users where Id = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, user_id as i64))?;
        if let State::Row = statement.next()? {
            let col_typee = statement.column_type("Storage")?;
            if let Type::Null = col_typee {
                return Ok(Default::default());
            }
            let storage = statement.read::<String, _>("Storage")?;
            let storage = serde_json::from_str::<AccountStorages>(&storage)?;
            return Ok(storage);
        }
        Ok(Default::default())
    }
    pub fn update_inventory(
        &mut self,
        char_id: u32,
        user_id: u32,
        inv: &Inventory,
    ) -> Result<(), Error> {
        let inventory = serde_json::to_string(&inv)?;
        let storage = serde_json::to_string(&inv.storages)?;
        let query = "update Characters set Inventory = ? where Id = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind::<&[(_, Value)]>(
            &[(1, inventory.as_str().into()), (2, (char_id as i64).into())][..],
        )?;
        statement.into_iter().count();
        let query = "update Users set Storage = ? where Id = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind::<&[(_, Value)]>(
            &[(1, storage.as_str().into()), (2, (user_id as i64).into())][..],
        )?;
        statement.into_iter().count();
        Ok(())
    }
    pub fn get_uuid(&self) -> Result<u64, Error> {
        let query = "select Value from ServerStats where Tag = \"UUID\"";
        let mut statement = self.connection.prepare(query)?;
        if let State::Row = statement.next()? {
            let col_typee = statement.column_type("Value")?;
            if let Type::Null = col_typee {
                return Ok(1);
            }
            let uuid = statement.read::<String, _>("Value")?;
            let uuid = uuid.parse().unwrap();
            Ok(uuid)
        } else {
            let query = "insert into ServerStats (Tag, Value) values (\"UUID\", 1)";
            self.connection.execute(query)?;
            Ok(1)
        }
    }
    pub fn set_uuid(&mut self, uuid: u64) -> Result<(), Error> {
        let query = "update ServerStats set Value = ? where Tag = \"UUID\"";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, uuid as i64))?;
        statement.into_iter().count();
        Ok(())
    }
    pub fn get_palette(&self, char_id: u32) -> Result<Palette, Error> {
        let query = "select Palette from Characters where Id = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind((1, char_id as i64))?;
        if let State::Row = statement.next()? {
            let col_typee = statement.column_type("Palette")?;
            if let Type::Null = col_typee {
                println!("wtf??");
                return Ok(Default::default());
            }
            let palette = statement.read::<String, _>("Palette")?;
            let palette = serde_json::from_str::<Palette>(&palette)?;
            return Ok(palette);
        }
        Ok(Default::default())
    }
    pub fn update_palette(&mut self, char_id: u32, palette: &Palette) -> Result<(), Error> {
        let palette = serde_json::to_string(&palette)?;
        let query = "update Characters set Palette = ? where Id = ?";
        let mut statement = self.connection.prepare(query)?;
        statement.bind::<&[(_, Value)]>(
            &[(1, palette.as_str().into()), (2, (char_id as i64).into())][..],
        )?;
        statement.into_iter().count();
        Ok(())
    }
}
