use std::sync::{Arc, Mutex};

use serde::Serialize;

#[derive(Clone, Copy, Default, Debug, Serialize)]
pub struct ActivityStatus {
    pub running_games: usize,
    pub file_operation: bool,
    pub revision: u64,
}

#[derive(Clone)]
pub struct GameActivity {
    state: Arc<Mutex<ActivityStatus>>,
    notify: Arc<dyn Fn(ActivityStatus) + Send + Sync>,
}

pub struct ActivityLease {
    activity: GameActivity,
    game: bool,
}

impl GameActivity {
    pub fn new(notify: impl Fn(ActivityStatus) + Send + Sync + 'static) -> Self {
        Self {
            state: Arc::new(Mutex::new(ActivityStatus::default())),
            notify: Arc::new(notify),
        }
    }

    pub fn status(&self) -> Result<ActivityStatus, String> {
        self.state.lock().map(|status| *status).map_err(|_| {
            "ゲームの実行状態を確認できません。ランチャーを再起動してください。".to_string()
        })
    }

    pub fn begin_launch(&self) -> Result<ActivityLease, String> {
        self.begin(true)
    }

    pub fn begin_file_operation(&self) -> Result<ActivityLease, String> {
        self.begin(false)
    }

    fn begin(&self, game: bool) -> Result<ActivityLease, String> {
        let status = {
            let mut status = self
                .state
                .lock()
                .map_err(|_| "ゲームの実行状態を確認できません".to_string())?;
            if status.file_operation {
                return Err(
                    "設定・バックアップなどのデータ操作が完了するまでお待ちください。".into(),
                );
            }
            if !game && status.running_games > 0 {
                return Err("Minecraftをすべて終了してからデータを操作してください。".into());
            }
            if game {
                status.running_games += 1;
            } else {
                status.file_operation = true;
            }
            status.revision += 1;
            *status
        };
        (self.notify)(status);
        Ok(ActivityLease {
            activity: self.clone(),
            game,
        })
    }
}

impl Drop for ActivityLease {
    fn drop(&mut self) {
        let status = match self.activity.state.lock() {
            Ok(mut status) => {
                if self.game {
                    status.running_games -= 1;
                } else {
                    status.file_operation = false;
                }
                status.revision += 1;
                *status
            }
            Err(err) => {
                eprintln!("failed to release game activity: {err}");
                return;
            }
        };
        (self.activity.notify)(status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_file_operations_until_all_games_exit() {
        let state = GameActivity::new(|_| {});
        let first = state.begin_launch().unwrap();
        let second = state.begin_launch().unwrap();
        assert!(state.begin_file_operation().is_err());
        drop(first);
        assert!(state.begin_file_operation().is_err());
        drop(second);
        assert!(state.begin_file_operation().is_ok());
    }

    #[test]
    fn excludes_launches_and_other_data_operations_during_maintenance() {
        let state = GameActivity::new(|_| {});
        let lease = state.begin_file_operation().unwrap();
        assert!(state.begin_launch().is_err());
        assert!(state.begin_file_operation().is_err());
        drop(lease);
        assert!(state.begin_launch().is_ok());
    }

    #[test]
    fn notifies_when_failed_preparation_releases_its_lease() {
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let events = statuses.clone();
        let state = GameActivity::new(move |status| events.lock().unwrap().push(status));
        let lease = state.begin_launch().unwrap();
        drop(lease);
        let statuses = statuses.lock().unwrap();
        assert_eq!(statuses[0].running_games, 1);
        assert_eq!(statuses[1].running_games, 0);
    }
}
