use pso2packetlib::protocol::login::ShipEntry;
use pso2server::{map::Map, sql, Action, BlockInfo, User};
use rsa::{pkcs8::EncodePrivateKey, RsaPrivateKey};
use std::{
    cell::RefCell,
    error, io,
    net::{Ipv4Addr, TcpListener},
    rc::Rc,
    sync::{Arc, Mutex, RwLock},
    thread::{self, Builder},
    time::Duration,
};

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
    let server_statuses = Arc::new(Mutex::new(Vec::<BlockInfo>::new()));
    let ship_statuses = Arc::new(RwLock::new(Vec::<ShipEntry>::new()));
    {
        let mut ships = ship_statuses.write().unwrap();
        ships.push(ShipEntry {
            id: 0,
            name: "Ship01".to_string(),
            ip: Ipv4Addr::UNSPECIFIED,
            status: pso2packetlib::protocol::login::ShipStatus::Online,
            order: 0,
        });
    }
    {
        let status_copy = ship_statuses;
        let querry = thread::spawn(move || querry_srv(status_copy));
        let status_copy = server_statuses.clone();
        let block_balance = thread::spawn(move || block_balance(status_copy));
        let status_copy = server_statuses;
        let server = Builder::new()
            .name("block".into())
            .stack_size(32 * 1024 * 1024)
            .spawn(move || init_srv(status_copy))?;

        println!("Server started.");
        while !querry.is_finished() || !block_balance.is_finished() || !server.is_finished() {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    Ok(())
}

fn init_srv(server_statuses: Arc<Mutex<Vec<BlockInfo>>>) -> Result<(), pso2server::Error> {
    let sql = Arc::new(RwLock::new(sql::Sql::new().unwrap()));
    let listener = TcpListener::bind("0.0.0.0:0")?;
    let name = "Block 01".to_string();
    listener.set_nonblocking(true)?;
    {
        let mut servers = server_statuses.lock().unwrap();
        servers.push(BlockInfo {
            ip: [0, 0, 0, 0],
            id: 1,
            name: name.clone(),
            port: listener.local_addr()?.port(),
        });
    }

    let lobby = match Map::new("lobby.mp") {
        Ok(x) => Rc::new(RefCell::new(x)),
        Err(e) => {
            eprintln!("Failed to load lobby map: {}", e);
            return Err(e);
        }
    };

    let mut clients = vec![];
    let mut to_remove = vec![];
    let mut actions = vec![];

    loop {
        for stream in listener.incoming() {
            match stream {
                Ok(s) => {
                    println!("Client connected");
                    clients.push(User::new(s, sql.clone(), name.clone())?);
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
        for (pos, client) in clients.iter_mut().enumerate() {
            if let Some(action) = handle_error(client.tick(), &mut to_remove, pos) {
                actions.push((action, pos));
            }
        }
        for (action, pos) in actions.drain(..) {
            match action {
                Action::Nothing => {}
                Action::LoadLobby => {
                    let user = &mut clients[pos];
                    let id = user.get_user_id();
                    user.set_map(lobby.clone());
                    handle_error(
                        lobby.borrow_mut().add_player(&mut clients, id),
                        &mut to_remove,
                        pos,
                    );
                }
                Action::SendPosition(packet) => {
                    let user = &clients[pos];
                    let id = user.get_user_id();
                    if let Some(map) = user.get_current_map() {
                        map.borrow_mut().send_movement(&mut clients, packet, id);
                    }
                }
                Action::SendMapMessage(packet) => {
                    let user = &clients[pos];
                    let id = user.get_user_id();
                    if let Some(map) = user.get_current_map() {
                        map.borrow_mut().send_message(&mut clients, packet, id);
                    }
                }
                Action::SendMapSA(packet) => {
                    let user = &clients[pos];
                    let id = user.get_user_id();
                    if let Some(map) = user.get_current_map() {
                        map.borrow_mut().send_sa(&mut clients, packet, id);
                    }
                }
            }
        }
        to_remove.sort_unstable();
        to_remove.dedup();
        for pos in to_remove.drain(..).rev() {
            println!("Client disconnected");
            let user = &clients[pos];
            let id = user.get_user_id();
            if let Some(map) = user.get_current_map() {
                map.borrow_mut().remove_player(&mut clients, id);
            }
            clients.remove(pos);
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn handle_error<T>(
    result: Result<T, pso2server::Error>,
    to_remove: &mut Vec<usize>,
    pos: usize,
) -> Option<T> {
    match result {
        Ok(t) => Some(t),
        Err(pso2server::Error::IOError(x)) if x.kind() == io::ErrorKind::ConnectionAborted => {
            to_remove.push(pos);
            None
        }
        Err(pso2server::Error::IOError(x)) if x.kind() == io::ErrorKind::WouldBlock => None,
        Err(x) => {
            to_remove.push(pos);
            eprintln!("Client error: {x}");
            eprintln!("Client error: {x:?}");
            None
        }
    }
}

fn block_balance(server_statuses: Arc<Mutex<Vec<BlockInfo>>>) -> io::Result<()> {
    // TODO: add ship id config
    let mut listeners = vec![];
    for i in 0..10 {
        //pc balance
        listeners.push(TcpListener::bind(("0.0.0.0", 12100 + (i * 100)))?);
        //vita balance
        listeners.push(TcpListener::bind(("0.0.0.0", 12193 + (i * 100)))?);
    }
    listeners
        .iter_mut()
        .map(|x| x.set_nonblocking(true).unwrap())
        .count();
    loop {
        for info_listener in &listeners {
            for stream in info_listener.incoming() {
                match stream {
                    Ok(s) => {
                        let _ = pso2server::send_block_balance(s, server_statuses.clone());
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
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

fn querry_srv(server_statuses: Arc<RwLock<Vec<ShipEntry>>>) -> io::Result<()> {
    let mut info_listeners: Vec<TcpListener> = vec![];
    for i in 0..10 {
        //pc ships
        info_listeners.push(TcpListener::bind(("0.0.0.0", 12199 + (i * 100)))?);
        //vita ships
        info_listeners.push(TcpListener::bind(("0.0.0.0", 12194 + (i * 100)))?);
    }
    info_listeners
        .iter_mut()
        .map(|x| x.set_nonblocking(true).unwrap())
        .count();
    loop {
        for info_listener in &info_listeners {
            for stream in info_listener.incoming() {
                match stream {
                    Ok(s) => {
                        let _ = pso2server::send_querry(s, server_statuses.clone());
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        break;
                    }
                    Err(e) => {
                        eprintln!("Querry error: {}", e);
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(1));
            }
        }
    }
}
