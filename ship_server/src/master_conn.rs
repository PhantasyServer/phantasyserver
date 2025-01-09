use crate::Error;
use data_structs::master_ship::{
    MasterShipAction as MAS, MasterShipComm, RegisterShipResult, SerializerFormat, ShipConnection,
    ShipInfo, ShipLogin, ShipLoginResult,
};
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::atomic::AtomicU32,
};
use tokio::sync::mpsc::{Receiver, Sender};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct HostKeyStorage {
    keys: Vec<HostKey>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct HostKey {
    ip: Ipv4Addr,
    fingerprint: String,
}

struct MasterConnectionImpl {
    id: u32,
    conn: ShipConnection,
    receive_ch: Receiver<(MAS, Sender<MAS>)>,
    action_ch: Receiver<ConnectionAction>,
    notif_ch: Sender<MAS>,
}

pub struct MasterConnection {
    send_ch: Sender<(MAS, Sender<MAS>)>,
    action_ch: Sender<ConnectionAction>,
    notif_cf: Option<Receiver<MAS>>,
    local_addr: Ipv4Addr,
    ship_id: AtomicU32,
}

enum ConnectionAction {
    SetFormat(SerializerFormat),
}

fn hostkey_fingerprint(key: &[u8]) -> String {
    use base64::Engine;
    use sha2::Digest;

    let mut hasher = sha2::Sha256::new();
    hasher.update(key);
    let hash = hasher.finalize();
    base64::engine::general_purpose::STANDARD.encode(hash)
}

fn ident_failure(fingerprint: &str) {
    log::warn!("@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@");
    log::warn!("@    WARNING: MASTER SERVER IDENTIFICATION HAS CHANGED!    @");
    log::warn!("@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@");
    log::warn!("IT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!");
    log::warn!("Someone could be eavesdropping on you right now (man-in-the-middle attack)!");
    log::warn!("It is also possible that a host key has just been changed.");
    log::warn!("The fingerprint for the key sent by the master server is");
    log::warn!("SHA256:{fingerprint}");
}

impl MasterConnection {
    pub async fn new(ip: SocketAddr, psk: &[u8], key_file: &str) -> Result<Self, Error> {
        let socket = tokio::net::TcpStream::connect(ip).await?;
        let IpAddr::V4(local_addr) = socket.local_addr()?.ip() else {
            unimplemented!()
        };
        let mut hostkeys: HostKeyStorage = toml::from_str(
            &tokio::fs::read_to_string(key_file)
                .await
                .unwrap_or_default(),
        )
        .unwrap_or_default();
        let conn = ShipConnection::new_client(socket, |ip, key| {
            let fingerprint = hostkey_fingerprint(key);
            if let Some(host) = hostkeys.keys.iter().find(|d| d.ip == ip) {
                match host.fingerprint == fingerprint {
                    true => return true,
                    false => {
                        ident_failure(&fingerprint);
                        return false;
                    }
                }
            }
            log::warn!(
                "The authenticity of master server '{}' can't be established.",
                local_addr
            );
            log::warn!("Key fingerprint is SHA256:{fingerprint}",);
            let confirm =
                dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt("Are you sure you want to continue connecting?")
                    .interact()
                    .unwrap();
            if confirm {
                hostkeys.keys.push(HostKey { ip, fingerprint });
                log::warn!(
                    "Permanently added '{}' to the list of known master ships.",
                    local_addr
                );
                true
            } else {
                false
            }
        })
        .await?;
        tokio::fs::write(
            "hostkeys.toml",
            toml::to_string_pretty(&hostkeys)?.as_bytes(),
        )
        .await?;
        let (send, recv) = tokio::sync::mpsc::channel(10);
        let (ac_send, ac_recv) = tokio::sync::mpsc::channel(10);
        let (notif_send, notif_recv) = tokio::sync::mpsc::channel(10);
        let master_conn = Self {
            send_ch: send,
            local_addr,
            ship_id: 0.into(),
            action_ch: ac_send,
            notif_cf: Some(notif_recv),
        };

        let master_conn_impl = MasterConnectionImpl {
            id: 1,
            conn,
            receive_ch: recv,
            action_ch: ac_recv,
            notif_ch: notif_send,
        };
        tokio::spawn(async move { master_conn_impl.run_loop().await });

        let response = master_conn
            .run_action(MAS::ShipLogin(ShipLogin { psk: psk.to_vec() }))
            .await?;
        match response {
            MAS::ShipLoginResult(ShipLoginResult::Ok) => Ok(master_conn),
            MAS::ShipLoginResult(ShipLoginResult::UnknownShip) => Err(Error::MSInvalidPSK),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn run_action(&self, action: MAS) -> Result<MAS, Error> {
        log::trace!("Request to master ship: {action:?}");
        let (send, mut recv) = tokio::sync::mpsc::channel(1);
        self.send_ch
            .send((action, send))
            .await
            .expect("Channel shouldn't be closed");
        match recv.recv().await {
            Some(d) => Ok(d),
            None => Err(Error::MSNoResponse),
        }
    }
    async fn try_format(&self, format: SerializerFormat) -> Result<bool, Error> {
        match self.run_action(MAS::SetFormat(format)).await? {
            MAS::Ok => Ok(true),
            MAS::Error(_) => Ok(false),
            _ => Err(Error::MSUnexpected),
        }
    }
    async fn try_formats(&self, formats: &[SerializerFormat]) -> Result<(), Error> {
        for f in formats {
            if self.try_format(f.clone()).await? {
                self.action_ch
                    .send(ConnectionAction::SetFormat(f.clone()))
                    .await
                    .unwrap();
                return Ok(());
            }
        }
        Ok(())
    }
    pub async fn register_ship(&self, mut info: ShipInfo) -> Result<RegisterShipResult, Error> {
        // use SerializerFormat as SF;
        // self.try_formats(&[SF::MessagePackUnnamed, SF::MessagePack])
        //     .await?;
        self.ship_id
            .swap(info.id, std::sync::atomic::Ordering::Relaxed);
        info.ip = self.local_addr;
        match self.run_action(MAS::RegisterShip(info)).await? {
            MAS::RegisterShipResult(x) => Ok(x),
            MAS::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub fn take_notif_ch(&mut self) -> Option<Receiver<MAS>> {
        self.notif_cf.take()
    }
}

impl MasterConnectionImpl {
    async fn run_loop(mut self) {
        let mut channels: Vec<(u32, Sender<MAS>)> = vec![];
        loop {
            tokio::select! {
                result = self.conn.read() => {
                    let result = match result {
                        Ok(r) => r,
                        Err(e) => {
                            log::error!("Failed to receive data from a master server: {e}");
                            return
                        }
                    };
                    if result.id == 0 {
                        log::trace!("Master ship notification: {result:?}");
                        let _ = self.notif_ch.send(result.action).await;
                    } else {
                        let Some((pos, _)) = channels.iter().enumerate().find(|(_, (id,_))| *id == result.id) else {
                            log::error!("Master server sent unhandled response: {result:?}");
                            return;
                        };
                        log::trace!("Master ship sent: {result:?}");
                        let (_, ch) = channels.swap_remove(pos);
                        let _ = ch.send(result.action).await;
                    }
                },
                Some((action, chan)) = self.receive_ch.recv() => {
                    let id = self.id;
                    self.id += 1;
                    match self.conn.write(MasterShipComm { id, action }).await {
                        Ok(_) => channels.push((id, chan)),
                        Err(e) => log::error!("Failed to send a request to a master server: {e}"),
                    }
                },
                Some(action) = self.action_ch.recv() => {
                    match action {
                        ConnectionAction::SetFormat(ac) => self.conn.set_format(ac),
                    }
                }
            }
        }
    }
}

impl Drop for MasterConnection {
    fn drop(&mut self) {
        let ship_id = self.ship_id.load(std::sync::atomic::Ordering::Relaxed);
        if ship_id != 0 {
            let (send, _) = tokio::sync::mpsc::channel(1);
            self.send_ch
                .try_send((MAS::UnregisterShip(ship_id), send))
                .expect("Channel shouldn't be closed");
        }
    }
}
