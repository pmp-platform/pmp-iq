//! Integration test for the Redis distributed-lock backend against a real Redis
//! container (exercises `RedisClientImpl` + `RedisLock` end to end).

use platform_inspector::jobs::SystemClock;
use platform_inspector::jobs::clock::Clock;
use platform_inspector::locks::{DistributedLock, LockError, RedisClientImpl, RedisLock};
use std::sync::Arc;
use std::time::Duration;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::redis::Redis;

async fn lock_for(url: &str) -> RedisLock {
    let client = RedisClientImpl::connect(url).expect("open redis client");
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    RedisLock::new(Arc::new(client), clock)
}

#[tokio::test]
async fn acquire_blocks_second_holder_then_refresh_and_release() {
    let container = Redis::default().start().await.expect("start redis");
    let port = container.get_host_port_ipv4(6379).await.expect("map redis port");
    let url = format!("redis://127.0.0.1:{port}");
    let lock = lock_for(&url).await;
    let key = "repo:org/api";

    // First holder acquires.
    let lease = lock
        .acquire(key, Duration::from_secs(30))
        .await
        .unwrap()
        .expect("granted");
    // A second acquisition while held is refused.
    assert!(lock.acquire(key, Duration::from_secs(30)).await.unwrap().is_none());

    // Refresh keeps the lease; release frees the key.
    let renewed = lock.refresh(&lease, Duration::from_secs(30)).await.unwrap();
    lock.release(&renewed).await.unwrap();
    assert!(lock.acquire(key, Duration::from_secs(30)).await.unwrap().is_some());
}

#[tokio::test]
async fn refresh_after_release_reports_lost() {
    let container = Redis::default().start().await.expect("start redis");
    let port = container.get_host_port_ipv4(6379).await.expect("map redis port");
    let url = format!("redis://127.0.0.1:{port}");
    let lock = lock_for(&url).await;

    let lease = lock
        .acquire("k", Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    lock.release(&lease).await.unwrap();
    // The key is gone, so a refresh by the old token is Lost.
    assert!(matches!(
        lock.refresh(&lease, Duration::from_secs(30)).await,
        Err(LockError::Lost(_))
    ));
}

#[tokio::test]
async fn different_keys_are_independent() {
    let container = Redis::default().start().await.expect("start redis");
    let port = container.get_host_port_ipv4(6379).await.expect("map redis port");
    let url = format!("redis://127.0.0.1:{port}");
    let lock = lock_for(&url).await;

    let a = lock.acquire("repo:a", Duration::from_secs(30)).await.unwrap();
    let b = lock.acquire("repo:b", Duration::from_secs(30)).await.unwrap();
    assert!(a.is_some() && b.is_some(), "distinct keys both acquire");
}
