#[tokio::main]
async fn main() {
    match master_ship::run().await {
        Ok(_) => {}
        Err(e) => eprintln!("Master ship error: {e}"),
    }
}
