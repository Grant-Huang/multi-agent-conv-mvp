//! 会话级串行：同一会话同一时刻只处理一条消息（先来先得），不同会话互不影响。
//!
//! 否则用户连发两条消息时，两轮会各自读同一份历史、各自决策，互相看不到对方的结果。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

#[derive(Default)]
pub struct ConvLocks {
    map: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

/// 持有期间，同一会话的其他消息在 `ConvLocks::lock` 处排队；丢弃时放行下一条
pub struct ConvGuard<'a> {
    locks: &'a ConvLocks,
    id: String,
    // Option 是为了先放锁、再清理 map（`Drop` 里不能 move 字段）
    guard: Option<OwnedMutexGuard<()>>,
    /// 排队等了多久（毫秒），0 表示没有排队
    pub waited_ms: u64,
}

impl ConvLocks {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn lock(&self, conversation_id: &str) -> ConvGuard<'_> {
        let entry = {
            let mut m = self.map.lock().unwrap_or_else(|e| e.into_inner());
            m.entry(conversation_id.to_string()).or_default().clone()
        };
        let started = std::time::Instant::now();
        let guard = entry.lock_owned().await;
        ConvGuard { locks: self, id: conversation_id.to_string(), guard: Some(guard), waited_ms: started.elapsed().as_millis() as u64 }
    }

    /// 当前有锁记录的会话数（测试用，确认不会无限增长）
    pub fn len(&self) -> usize {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Drop for ConvGuard<'_> {
    fn drop(&mut self) {
        drop(self.guard.take());
        // 没有别人在等（map 里只剩自己这一份引用）就清理掉，避免会话越多内存越大
        let mut m = self.locks.map.lock().unwrap_or_else(|e| e.into_inner());
        if m.get(&self.id).is_some_and(|a| Arc::strong_count(a) == 1) {
            m.remove(&self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn same_conversation_is_serialized_in_arrival_order() {
        let locks = Arc::new(ConvLocks::new());
        let log = Arc::new(Mutex::new(vec![]));
        let first = locks.lock("c1").await;
        let mut handles = vec![];
        for i in 0..3 {
            let (l, lg) = (locks.clone(), log.clone());
            handles.push(tokio::spawn(async move {
                let g = l.lock("c1").await;
                lg.lock().unwrap().push(format!("start{i}"));
                tokio::time::sleep(Duration::from_millis(20)).await;
                lg.lock().unwrap().push(format!("end{i}"));
                g.waited_ms
            }));
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        drop(first);
        let mut waits = vec![];
        for h in handles {
            waits.push(h.await.unwrap());
        }
        assert_eq!(*log.lock().unwrap(), ["start0", "end0", "start1", "end1", "start2", "end2"]);
        assert!(waits.iter().all(|w| *w > 0), "{waits:?}");
    }

    #[tokio::test]
    async fn different_conversations_do_not_block_each_other() {
        let locks = ConvLocks::new();
        let _a = locks.lock("c1").await;
        let b = tokio::time::timeout(Duration::from_millis(200), locks.lock("c2")).await;
        assert!(b.is_ok());
    }

    #[tokio::test]
    async fn entries_are_cleaned_up_after_use() {
        let locks = Arc::new(ConvLocks::new());
        {
            let g = locks.lock("c1").await;
            assert_eq!((g.waited_ms, locks.len()), (0, 1));
        }
        assert!(locks.is_empty());
        // 有人排队时不清理，全部结束后才清理
        let g1 = locks.lock("c1").await;
        let l2 = locks.clone();
        let waiter = tokio::spawn(async move { drop(l2.lock("c1").await) });
        tokio::time::sleep(Duration::from_millis(30)).await;
        drop(g1);
        waiter.await.unwrap();
        assert!(locks.is_empty());
    }
}
