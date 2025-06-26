use psyche_rs::pete;
use tokio::task::LocalSet;
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn launching_default_pete_completes() {
    let local = LocalSet::new();
    let res = timeout(
        Duration::from_secs(2),
        local.run_until(pete::launch_default_pete()),
    )
    .await;
    assert!(res.expect("execution timed out").is_ok());
}
