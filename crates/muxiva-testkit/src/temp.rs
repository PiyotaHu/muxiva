use std::{
    fs,
    net::{SocketAddr, TcpListener},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
pub struct TestDirectory(PathBuf);
impl TestDirectory {
    pub fn new(label: &str) -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "muxiva-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub struct ReservedPort(TcpListener);
impl ReservedPort {
    pub fn loopback() -> std::io::Result<Self> {
        TcpListener::bind(("127.0.0.1", 0)).map(Self)
    }
    pub fn address(&self) -> std::io::Result<SocketAddr> {
        self.0.local_addr()
    }
    pub fn listener(&self) -> &TcpListener {
        &self.0
    }
}
