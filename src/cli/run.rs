use anyhow::Result;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

use crate::config::Config;
use crate::db::Db;
use crate::manager;
use crate::worker;

pub fn execute(workers: usize) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = cwd.join(".acs");
    if !acs_dir.exists() {
        anyhow::bail!(".acs/ not found. Run `acs init` first.");
    }

    let config = Config::load(&acs_dir.join("config.toml"))?;
    let db = Arc::new(Mutex::new(Db::open(&acs_dir.join("project.db"))?));

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Register workers
        {
            let db = db.lock().unwrap();
            for i in 0..workers {
                let worker_id = format!("w-{}", i);
                db.register_agent(&worker_id, "worker", "general")?;
            }
        }

        // Spawn manager task
        let mgr_db = db.clone();
        let mgr_config = config.clone();
        let mgr_shutdown = shutdown_rx.clone();
        let mgr_dir = cwd.clone();
        let mgr_handle = tokio::spawn(async move {
            manager::run_loop(mgr_db, &mgr_config, mgr_dir, mgr_shutdown).await
        });

        // Spawn worker tasks
        let mut worker_handles = vec![];
        for i in 0..workers {
            let worker_id = format!("w-{}", i);
            let w_db = db.clone();
            let w_config = config.clone();
            let w_dir = cwd.clone();
            let w_shutdown = shutdown_rx.clone();
            let handle = tokio::spawn(async move {
                worker::worker_loop(worker_id, w_db, w_config, w_dir, w_shutdown).await
            });
            worker_handles.push(handle);
        }

        println!("ACS running with {} workers. Press Ctrl+C to stop.", workers);

        // Wait for Ctrl+C
        tokio::signal::ctrl_c().await.ok();
        println!("\nShutting down...");
        shutdown_tx.send(true).ok();

        // Wait for all tasks
        mgr_handle.await.ok();
        for h in worker_handles {
            h.await.ok();
        }

        // Cleanup
        {
            let db = db.lock().unwrap();
            for i in 0..workers {
                db.deregister_agent(&format!("w-{}", i)).ok();
            }
        }

        println!("Stopped.");
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
