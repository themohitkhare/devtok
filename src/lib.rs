pub mod cli;
pub mod config;
pub mod db;
pub mod manager;
pub mod models;
pub mod prompts;
pub mod quality;
pub mod release;
pub mod spawner;
pub mod worker;

#[cfg(test)]
pub mod test_support {
    use std::sync::{Mutex, MutexGuard};

    static CHDIR_LOCK: Mutex<()> = Mutex::new(());

    pub fn chdir_lock() -> MutexGuard<'static, ()> {
        CHDIR_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }
}
