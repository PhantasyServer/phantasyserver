use console::style;
use data_structs::ItemParameters;
use indicatif::{MultiProgress, ProgressBar};
use parking_lot::RwLock;
use pso2packetlib::protocol::login::ShipEntry;
use pso2ship_server::{init_block, sql, BlockInfo};
use rsa::{pkcs8::EncodePrivateKey, RsaPrivateKey};
use std::{
    error, io,
    net::{Ipv4Addr, TcpListener},
    sync::Arc,
    thread,
    time::Duration,
};

fn main() -> Result<(), Box<dyn error::Error>> {
    let mul_progress = MultiProgress::new();
    let startup_progress = mul_progress.add(ProgressBar::new_spinner());
    startup_progress.set_message("Starting server...");
    match std::fs::metadata("keypair.pem") {
        Ok(..) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let key_progress = mul_progress.add(ProgressBar::new_spinner());
            key_progress.set_message(style("No keyfile found, creating...").yellow().to_string());
            let mut rand_gen = rand::thread_rng();
            let key = RsaPrivateKey::new(&mut rand_gen, 1024)?;
            key.write_pkcs8_pem_file("keypair.pem", rsa::pkcs8::LineEnding::default())?;
            key_progress.finish_with_message("Keyfile created.")
        }
        Err(e) => {
            return Err(e.into());
        }
    }
    let (data_pc, data_vita) = pso2ship_server::create_attr_files(&mul_progress)?;
    let mut item_data = ItemParameters::load_from_mp_file("names.mp")?;
    item_data.pc_attrs = data_pc;
    item_data.vita_attrs = data_vita;
    let item_data = Arc::new(RwLock::new(item_data));
    let server_statuses = Arc::new(RwLock::new(Vec::<BlockInfo>::new()));
    let ship_statuses = Arc::new(RwLock::new(Vec::<ShipEntry>::new()));
    {
        let mut ships = ship_statuses.write();
        ships.push(ShipEntry {
            id: 1000,
            name: "Ship01".to_string(),
            ip: Ipv4Addr::UNSPECIFIED,
            status: pso2packetlib::protocol::login::ShipStatus::Online,
            order: 1,
        });
        ships.push(ShipEntry {
            id: 2000,
            name: "Ship02".to_string(),
            ip: Ipv4Addr::UNSPECIFIED,
            status: pso2packetlib::protocol::login::ShipStatus::Online,
            order: 2,
        });
    }
    {
        let sql = Arc::new(RwLock::new(sql::Sql::new().unwrap()));
        let status_copy = ship_statuses;
        let querry = thread::spawn(move || querry_srv(status_copy));
        let status_copy = server_statuses.clone();
        let block_balance = thread::spawn(move || block_balance(status_copy));
        let mut blocks = vec![];
        for i in 100..101 {
            let mut blockstatus_lock = server_statuses.write();
            let new_block = BlockInfo {
                id: i,
                name: format!("Block {}", i),
                ip: [0, 0, 0, 0],
                port: 13000 + i as u16,
            };
            blockstatus_lock.push(new_block.clone());
            let server_statuses = server_statuses.clone();
            let sql = sql.clone();
            let item_data = item_data.clone();
            blocks.push(thread::spawn(move || {
                init_block(server_statuses, new_block, sql, item_data)
            }))
        }

        startup_progress.finish_with_message("Server started.");
        while !querry.is_finished() || !block_balance.is_finished() {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    Ok(())
}

fn block_balance(server_statuses: Arc<RwLock<Vec<BlockInfo>>>) -> io::Result<()> {
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
                        let _ = pso2ship_server::send_block_balance(s, server_statuses.clone());
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
                        let _ = pso2ship_server::send_querry(s, server_statuses.clone());
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        break;
                    }
                    Err(e) => {
                        eprintln!("Querry error: {}", e);
                        break;
                    }
                }
            }
        }
    }
}
