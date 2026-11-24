use easytier::common::config::{ConfigFileControl, ConfigLoader, TomlConfigLoader};
use easytier::instance::factory::native_cli_instance_manager;
use std::collections::BTreeMap;
use std::io::{self, BufRead};
use uuid::Uuid;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    // Единственный канал управления — унаследованный от службы stdin.
    let manager = native_cli_instance_manager(true, 2)?;
    let mut running = BTreeMap::<Uuid, String>::new();
    for message in io::stdin().lock().lines() {
        let desired: BTreeMap<Uuid, String> = serde_json::from_str(&message?)?;
        let mut additions = Vec::new();
        for (id, text) in &desired {
            if running.get(id) == Some(text) {
                continue;
            }
            let config = TomlConfigLoader::new_from_str(text)?;
            anyhow::ensure!(config.get_id() == *id, "Mismatched network ID");
            additions.push((*id, config));
        }
        let removed: Vec<_> = running
            .iter()
            .filter(|(id, text)| desired.get(id) != Some(*text))
            .map(|(id, _)| *id)
            .collect();
        if !removed.is_empty() {
            manager.delete_network_instances(removed).await?;
        }
        for (_, config) in additions {
            manager.run_network_instance(config, ConfigFileControl::STATIC_CONFIG)?;
        }
        running = desired;
    }
    manager.retain_network_instances(&[]).await?;
    Ok(())
}
