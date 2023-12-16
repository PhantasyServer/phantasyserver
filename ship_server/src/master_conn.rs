use crate::Error;
use data_structs::{
    MasterShipAction as MAS, MasterShipComm, RegisterShipResult, ShipConnection, ShipInfo,
};
use parking_lot::{Mutex, MutexGuard};
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

#[derive(Serialize, Deserialize, Clone, Debug)]
struct HostKey {
    ip: Ipv4Addr,
    key: [u8; 32],
}

pub struct MasterConnection {
    id: u32,
    conn: ShipConnection,
    actions: Vec<(u32, MAS)>,
    local_addr: Ipv4Addr,
    ship_id: u32,
}

impl MasterConnection {
    pub async fn new(ip: SocketAddr) -> Result<Mutex<Self>, Error> {
        let socket = tokio::net::TcpStream::connect(ip).await?;
        let IpAddr::V4(local_addr) = socket.local_addr()?.ip() else {
            unimplemented!()
        };
        let mut hostkeys: Vec<HostKey> =
            rmp_serde::from_slice(&tokio::fs::read("hostkeys.mp").await.unwrap_or(vec![]))
                .unwrap_or(Default::default());
        let conn = ShipConnection::new_client(socket, |ip, key| {
            for entry in hostkeys.iter().filter(|d| d.ip == ip) {
                if &entry.key == key {
                    return true;
                } else {
                    return false;
                }
            }
            let key = key.to_owned();
            hostkeys.push(HostKey { ip, key });
            true
        })
        .await?;
        tokio::fs::write("hostkeys.mp", rmp_serde::to_vec(&hostkeys)?).await?;
        Ok(Mutex::new(Self {
            id: 0,
            conn,
            actions: vec![],
            local_addr,
            ship_id: 0,
        }))
    }
    pub async fn run_action(this: &Mutex<Self>, action: MAS) -> Result<MAS, Error> {
        let call_id = {
            let mut lock = async_lock(this).await;
            let id = lock.id;
            lock.id += 1;
            lock.conn.write(MasterShipComm { id, action }).await?;
            id
        };
        loop {
            let mut lock = async_lock(this).await;
            if let Some((pos, _)) = lock
                .actions
                .iter()
                .enumerate()
                .find(|(_, (id, _))| *id == call_id)
            {
                return Ok(lock.actions.swap_remove(pos).1);
            }
            match tokio::time::timeout(Duration::from_millis(10), lock.conn.read()).await {
                Ok(r) => {
                    let r = r?;
                    lock.actions.push((r.id, r.action));
                }
                Err(_) => {}
            }

            drop(lock);
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
    pub async fn register_ship(
        this: &Mutex<Self>,
        mut info: ShipInfo,
    ) -> Result<RegisterShipResult, Error> {
        {
            let mut lock = async_lock(this).await;
            lock.ship_id = info.id;
            info.ip = lock.local_addr;
        }
        match Self::run_action(this, MAS::RegisterShip(info)).await? {
            MAS::RegisterShipResult(x) => Ok(x),
            MAS::Error(e) => Err(Error::Generic(e)),
            _ => Err(Error::InvalidInput),
        }
    }
}

impl Drop for MasterConnection {
    fn drop(&mut self) {
        if self.ship_id != 0 {
            let _ = self.conn.write_blocking(MasterShipComm {
                id: self.ship_id,
                action: MAS::UnregisterShip(self.ship_id),
            });
        }
    }
}

async fn async_lock<T>(mutex: &Mutex<T>) -> MutexGuard<T> {
    loop {
        match mutex.try_lock() {
            Some(lock) => return lock,
            None => tokio::time::sleep(Duration::from_millis(1)).await,
        }
    }
}
