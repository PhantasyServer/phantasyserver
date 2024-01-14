use crate::{
    map,
    mutex::{Mutex, RwLock},
    party, sql,
    user::User,
    Action, BlockData, BlockInfo, Error,
};
use console::style;
use data_structs::ItemParameters;
use pso2packetlib::PrivateKey;
use std::{
    collections::HashMap,
    io,
    net::TcpListener,
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

pub async fn init_block(
    blocks: Arc<RwLock<Vec<BlockInfo>>>,
    this_block: BlockInfo,
    sql: Arc<sql::Sql>,
    item_attrs: Arc<RwLock<ItemParameters>>,
    key: PrivateKey,
) -> Result<(), Error> {
    let listener = TcpListener::bind(("0.0.0.0", this_block.port))?;
    listener.set_nonblocking(true)?;

    let latest_mapid = AtomicU32::new(0);

    let mut maps = HashMap::new();

    for (map_name, map_path) in this_block.maps {
        match map::Map::new(map_path, &latest_mapid) {
            Ok(x) => {
                maps.insert(map_name, Arc::new(Mutex::new(x)));
            }
            Err(e) => {
                eprintln!(
                    "{}",
                    style(format!("Failed to load map {}: {}", map_name, e)).red()
                );
            }
        }
    }

    let lobby = match maps.get("lobby") {
        Some(x) => x.clone(),
        None => return Err(Error::NoLobby),
    };

    let block_data = Arc::new(BlockData {
        sql,
        blocks,
        item_attrs,
        block_id: this_block.id,
        block_name: this_block.name,
        lobby,
        key,
        latest_mapid,
        latest_partyid: AtomicU32::new(0),
        quests: this_block.quests,
    });

    let mut clients = vec![];
    let mut conn_id = 0usize;
    let (send, recv) = mpsc::channel();

    loop {
        for stream in listener.incoming() {
            match stream {
                Ok(s) => {
                    println!("{}", style("Client connected").cyan());
                    let mut lock = block_data.blocks.write().await;
                    if let Some(block) = lock.iter_mut().find(|x| x.id == this_block.id) {
                        if block.players >= block.max_players {
                            continue;
                        }
                        block.players += 1;
                    }
                    drop(lock);
                    let client = Arc::new(Mutex::new(User::new(s, block_data.clone())?));
                    clients.push((conn_id, client.clone()));
                    let send = send.clone();
                    tokio::spawn(async move {
                        loop {
                            match User::tick(client.lock().await).await {
                                Ok(Action::Nothing) => {}
                                Ok(Action::Disconnect) => {
                                    send.send((conn_id, Action::Disconnect)).unwrap();
                                    return;
                                }
                                Ok(a) => {
                                    send.send((conn_id, a)).unwrap();
                                }
                                Err(Error::IOError(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                                }
                                Err(e) => {
                                    let error_msg = format!("Client error: {e}");
                                    let _ = client.lock().await.send_error(&error_msg);
                                    eprintln!("{}", style(error_msg).red());
                                }
                            }
                            tokio::time::sleep(Duration::from_millis(1)).await;
                        }
                    });

                    conn_id += 1;
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
        while let Ok((id, action)) = recv.try_recv() {
            match run_action(&mut clients, id, action, &block_data).await {
                Ok(_) => {}
                Err(e) => eprintln!("{}", style(format!("Client error: {e}")).red()),
            };
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

async fn run_action(
    clients: &mut Vec<(usize, Arc<Mutex<User>>)>,
    conn_id: usize,
    action: Action,
    block_data: &Arc<BlockData>,
) -> Result<(), Error> {
    let Some((pos, _)) = clients
        .iter()
        .enumerate()
        .find(|(_, (c_conn_id, _))| *c_conn_id == conn_id)
    else {
        return Ok(());
    };
    match action {
        Action::Nothing => {}
        Action::Disconnect => {
            println!("{}", style("Client disconnected").cyan());
            let mut lock = block_data.blocks.write().await;
            if let Some(block) = lock.iter_mut().find(|x| x.id == block_data.block_id) {
                block.players -= 1;
            }
            drop(lock);
            clients.remove(pos);
        }
        Action::InitialLoad => {
            let (_, user) = &clients[pos];
            let mut user_lock = user.lock().await;
            user_lock.set_map(block_data.lobby.clone());
            drop(user_lock);
            let party_id = block_data.latest_partyid.fetch_add(1, Ordering::Relaxed);
            party::Party::init_player(user.clone(), party_id).await?;
            block_data
                .lobby
                .lock()
                .await
                .init_add_player(user.clone())
                .await?;
        }
        Action::SendPartyInvite(invitee) => {
            let (_, inviter) = &clients[pos];
            let invitee = async {
                for client in clients.iter().map(|(_, p)| p) {
                    if client.lock().await.get_user_id() == invitee {
                        return Some(client.clone());
                    }
                }
                None
            }
            .await;
            if let Some(invitee) = invitee {
                party::Party::send_invite(inviter.clone(), invitee).await?;
            }
        }
    }
    Ok(())
}
