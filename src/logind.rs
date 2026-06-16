use zbus::{proxy, Connection, Result};

#[proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
pub trait Session {
    fn activate(&self) -> Result<()>;
    fn kill(&self, whom: &str, signal_number: i32) -> Result<()>;
    fn lock(&self) -> Result<()>;
    fn pause_device_complete(&self, major: u32, minor: u32) -> Result<()>;
    fn release_control(&self) -> Result<()>;
    fn release_device(&self, major: u32, minor: u32) -> Result<()>;
    fn set_brightness(&self, subsystem: &str, name: &str, brightness: u32) -> Result<()>;
    fn set_class(&self, class: &str) -> Result<()>;
    fn set_display(&self, display: &str) -> Result<()>;
    fn set_idle_hint(&self, idle: bool) -> Result<()>;
    fn set_locked_hint(&self, locked: bool) -> Result<()>;
    #[zbus(name = "SetTTY")]
    fn set_tty(&self, tty_fd: zbus::zvariant::Fd<'_>) -> Result<()>;
    fn set_type(&self, type_: &str) -> Result<()>;
    fn take_control(&self, force: bool) -> Result<()>;
    fn take_device(&self, major: u32, minor: u32) -> Result<(zbus::zvariant::OwnedFd, bool)>;
    fn terminate(&self) -> Result<()>;
    fn unlock(&self) -> Result<()>;

    #[zbus(signal, name = "Lock")]
    fn lock_signal(&self) -> Result<()>;

    #[zbus(signal, name = "Unlock")]
    fn unlock_signal(&self) -> Result<()>;

    #[zbus(property)]
    fn active(&self) -> Result<bool>;
    #[zbus(property)]
    fn id(&self) -> Result<String>;
    #[zbus(property)]
    fn locked_hint(&self) -> Result<bool>;
    #[zbus(property)]
    fn name(&self) -> Result<String>;
    #[zbus(property)]
    fn state(&self) -> Result<String>;
}

pub async fn get_session_proxy(conn: &Connection) -> zbus::Result<SessionProxy<'static>> {
    use zbus::{proxy::Builder, Proxy};

    let manager: Proxy = Builder::new(conn)
        .destination("org.freedesktop.login1")?
        .path("/org/freedesktop/login1")?
        .interface("org.freedesktop.login1.Manager")?
        .build()
        .await?;

    let session_id = std::env::var("XDG_SESSION_ID")
        .map_err(|_| zbus::Error::Failure("XDG_SESSION_ID not set".into()))?;
    let session_path: zbus::zvariant::OwnedObjectPath =
        manager.call("GetSession", &(session_id,)).await?;

    SessionProxy::builder(conn)
        .path(session_path)?
        .build()
        .await
}
