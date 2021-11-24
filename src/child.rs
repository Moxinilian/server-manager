use std::ops::{Deref, DerefMut};

use async_std::process::Child;

use crate::server::ServerManager;

pub struct ChildKiller(pub Child);

impl Drop for ChildKiller {
    fn drop(&mut self) {
        async_std::task::block_on(ServerManager::emergency_shutdown(&mut self.0));
    }
}

impl Deref for ChildKiller {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ChildKiller {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
