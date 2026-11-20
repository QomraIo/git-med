pub mod add;
pub mod branch;
pub mod clone;
pub mod commit;
pub mod init;
pub mod merge;
pub mod mix;
pub mod pull;
pub mod push;
pub mod remote;
pub mod tree;
pub mod view;

pub fn block_on<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(future))
    } else {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(future)
    }
}
