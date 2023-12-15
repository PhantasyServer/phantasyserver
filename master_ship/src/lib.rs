use data_structs::{Error, MasterShipAction, MasterShipComm};
use parking_lot::RwLock;
use pso2packetlib::{
    protocol::{login, Packet, PacketType},
    Connection,
};
use std::{io, net::Ipv4Addr, sync::Arc, time::Duration};
use tokio::net::{TcpListener, TcpStream};

type Ships = Arc<RwLock<Vec<login::ShipEntry>>>;

pub async fn ship_receiver(servers: Ships) -> io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", 15000)).await?;
    loop {
        match listener.accept().await {
            Ok((s, _)) => {
                let servers = servers.clone();
                tokio::spawn(async move {
                    let mut conn = data_structs::ShipConnection::new_server(s, &[1; 32])
                        .await
                        .unwrap();
                    loop {
                        match conn.read().await {
                            Ok(d) => match run_action(&servers, d).await {
                                Ok(_) => {}
                                Err(e) => eprintln!("Action error: {e}"),
                            },
                            Err(e) => {
                                eprintln!("Read error: {e}");
                                return;
                            }
                        }
                    }
                });
            }
            Err(e) => Err(e)?,
        }
    }
}

pub async fn run_action(ships: &Ships, action: MasterShipComm) -> Result<(), Error> {
    match action.action {
        MasterShipAction::RegisterShip => {}
    }
    Ok(())
}

pub async fn test_ship() -> Result<(), data_structs::Error> {
    tokio::time::sleep(Duration::from_secs(2)).await;
    let socket = TcpStream::connect(("127.0.0.1", 15000)).await?;
    let mut conn = data_structs::ShipConnection::new_client(socket, |_, _| true).await?;
    conn.write(data_structs::MasterShipComm {
        id: 0,
        action: data_structs::MasterShipAction::RegisterShip,
    })
    .await?;
    Ok(())
}

pub async fn make_query(servers: Arc<RwLock<Vec<login::ShipEntry>>>) -> io::Result<()> {
    let mut info_listeners: Vec<TcpListener> = vec![];
    for i in 0..10 {
        // pc ships
        info_listeners.push(TcpListener::bind(("0.0.0.0", 12199 + (i * 100))).await?);
        // vita ships
        info_listeners.push(TcpListener::bind(("0.0.0.0", 12194 + (i * 100))).await?);
    }
    for listener in info_listeners {
        let servers = servers.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((s, _)) => {
                        let _ = send_querry(s, servers.clone());
                    }
                    Err(e) => {
                        eprintln!("Failed to accept connection: {}", e);
                        return;
                    }
                }
            }
        });
    }
    Ok(())
}

fn send_querry(stream: TcpStream, servers: Arc<RwLock<Vec<login::ShipEntry>>>) -> io::Result<()> {
    stream.set_nodelay(true)?;
    let local_addr = stream.local_addr()?.ip();
    let mut con = Connection::new(stream.into_std()?, PacketType::Classic, None, None);
    let mut ships = vec![];
    for server in servers.read().iter() {
        let mut ship = server.clone();
        if ship.ip == Ipv4Addr::UNSPECIFIED {
            if let std::net::IpAddr::V4(addr) = local_addr {
                ship.ip = addr
            }
        }
        ships.push(ship);
    }
    con.write_packet(&Packet::ShipList(login::ShipListPacket {
        ships,
        ..Default::default()
    }))?;
    Ok(())
}
