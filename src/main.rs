use pso2server::{sql, ServerInfo, User};
use rsa::{pkcs8::EncodePrivateKey, RsaPrivateKey};
use std::{
    error, io,
    net::TcpListener,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::sleep;

fn main() -> Result<(), Box<dyn error::Error>> {
    match std::fs::metadata("keypair.pem") {
        Ok(..) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            println!("No keyfile found, creating...");
            let mut rand_gen = rand::thread_rng();
            let key = RsaPrivateKey::new(&mut rand_gen, 1024)?;
            key.write_pkcs8_pem_file("keypair.pem", rsa::pkcs8::LineEnding::default())?;
        }
        Err(e) => {
            return Err(e.into());
        }
    }
    let server_statuses = Arc::new(Mutex::new(Vec::<ServerInfo>::new()));

    let rt = tokio::runtime::Runtime::new()?;
    {
        let _guard = rt.enter();
        let querry = tokio::spawn(querry_srv(server_statuses.clone()));
        let block_balance = tokio::spawn(block_balance(server_statuses.clone()));
        let server = tokio::spawn(init_srv(server_statuses));
        println!("Server started.");

        while !querry.is_finished() || !block_balance.is_finished() || !server.is_finished() {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    Ok(())
}

async fn init_srv(server_statuses: Arc<Mutex<Vec<ServerInfo>>>) -> Result<(), pso2server::Error> {
    let sql = Arc::new(sql::Sql::new().unwrap());
    let listener = TcpListener::bind("0.0.0.0:12456")?;
    listener.set_nonblocking(true)?;
    {
        let mut servers = server_statuses.lock().unwrap();
        servers.push(ServerInfo {
            ip: [0, 0, 0, 0],
            id: 1,
            name: "Ship01".to_string(),
            port: 12456,
            order: 1,
            status: 1,
        });
        servers.push(ServerInfo {
            ip: [0, 0, 0, 0],
            id: 2,
            name: "Ship02".to_string(),
            port: 12456,
            order: 2,
            status: 1,
        });
    }

    let mut clients = vec![];
    let mut to_remove = vec![];

    loop {
        for stream in listener.incoming() {
            match stream {
                Ok(s) => {
                    println!("Client connected");
                    clients.push(User::new(s, sql.clone())?);
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
        for (pos, client) in clients.iter_mut().enumerate() {
            match client.tick() {
                Ok(_) => {}
                Err(pso2server::Error::IOError(x))
                    if x.kind() == io::ErrorKind::ConnectionAborted =>
                {
                    to_remove.push(pos)
                }
                Err(x) => {
                    to_remove.push(pos);
                    println!("Client error: {x}");
                }
            }
        }
        to_remove.sort_unstable();
        for pos in to_remove.drain(..).rev() {
            println!("Client disconnected");
            clients.remove(pos);
        }
        sleep(Duration::from_millis(1)).await;
    }
}

async fn block_balance(server_statuses: Arc<Mutex<Vec<ServerInfo>>>) -> io::Result<()> {
    let mut listeners = vec![
        TcpListener::bind("0.0.0.0:12193")?, //vita ship1
        TcpListener::bind("0.0.0.0:12100")?, //pc ship1
        TcpListener::bind("0.0.0.0:12293")?, //vita ship2
        TcpListener::bind("0.0.0.0:12200")?, //pc ship2
        TcpListener::bind("0.0.0.0:12181")?, //steam ship1
    ];
    listeners
        .iter_mut()
        .map(|x| x.set_nonblocking(true).unwrap())
        .count();
    loop {
        for info_listener in &listeners {
            for stream in info_listener.incoming() {
                match stream {
                    Ok(s) => {
                        tokio::spawn(pso2server::send_block_balance(s, server_statuses.clone()));
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        sleep(Duration::from_millis(1)).await;
                        break;
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
        }
    }
}

async fn querry_srv(server_statuses: Arc<Mutex<Vec<ServerInfo>>>) -> io::Result<()> {
    let mut info_listeners: Vec<TcpListener> = vec![
        TcpListener::bind("0.0.0.0:12199")?,
        TcpListener::bind("0.0.0.0:12180")?,
        TcpListener::bind("0.0.0.0:12280")?,
        TcpListener::bind("0.0.0.0:12299")?,
        TcpListener::bind("0.0.0.0:12194")?,
        TcpListener::bind("0.0.0.0:12294")?,
        TcpListener::bind("0.0.0.0:12394")?,
        TcpListener::bind("0.0.0.0:12494")?,
    ];
    info_listeners
        .iter_mut()
        .map(|x| x.set_nonblocking(true).unwrap())
        .count();
    loop {
        for info_listener in &info_listeners {
            for stream in info_listener.incoming() {
                match stream {
                    Ok(s) => {
                        tokio::spawn(pso2server::send_querry(s, server_statuses.clone()));
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        sleep(Duration::from_millis(1)).await;
                        break;
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
        }
    }
}
