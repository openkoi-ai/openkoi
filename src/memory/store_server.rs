// src/memory/store_server.rs — Async message passing for Store

use crate::memory::store::{LearningRow, Store, UsageEventRow, UsagePatternRow};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
pub enum StoreCommand {
    InsertSession {
        id: String,
        channel: String,
        model_provider: String,
        model_id: String,
        resp: oneshot::Sender<anyhow::Result<()>>,
    },
    UpdateSessionTotals {
        id: String,
        tokens: i64,
        cost: f64,
        resp: oneshot::Sender<anyhow::Result<()>>,
    },
    InsertTask {
        id: String,
        description: String,
        category: Option<String>,
        session_id: Option<String>,
        resp: oneshot::Sender<anyhow::Result<()>>,
    },
    CompleteTask {
        id: String,
        final_score: f64,
        iterations: i32,
        decision: String,
        total_tokens: i64,
        total_cost: f64,
        resp: oneshot::Sender<anyhow::Result<()>>,
    },
    InsertUsageEvent {
        id: String,
        event_type: String,
        channel: Option<String>,
        description: Option<String>,
        category: Option<String>,
        skills_used: Option<String>,
        score: Option<f64>,
        day: String,
        hour: Option<i32>,
        day_of_week: Option<i32>,
        resp: oneshot::Sender<anyhow::Result<()>>,
    },
    QueryEventsSince {
        since: String,
        resp: oneshot::Sender<anyhow::Result<Vec<UsageEventRow>>>,
    },
    QueryApprovedPatterns {
        resp: oneshot::Sender<anyhow::Result<Vec<UsagePatternRow>>>,
    },
    QuerySkillEffectiveness {
        skill_name: String,
        task_category: String,
        resp: oneshot::Sender<anyhow::Result<Option<crate::memory::store::SkillEffectivenessRow>>>,
    },
    QueryLearningsByType {
        learning_type: String,
        limit: u32,
        resp: oneshot::Sender<anyhow::Result<Vec<LearningRow>>>,
    },
    QueryTopSkillsForCategory {
        category: String,
        limit: u32,
        resp: oneshot::Sender<anyhow::Result<Vec<crate::memory::store::SkillEffectivenessRow>>>,
    },
    CountLearnings {
        resp: oneshot::Sender<anyhow::Result<usize>>,
    },
    QueryAllLearnings {
        resp: oneshot::Sender<anyhow::Result<Vec<LearningRow>>>,
    },
    ReinforceLearning {
        id: String,
        resp: oneshot::Sender<anyhow::Result<()>>,
    },
    InsertCycle {
        id: String,
        task_id: String,
        iteration: i32,
        score: Option<f64>,
        decision: String,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        duration_ms: Option<i64>,
        resp: oneshot::Sender<anyhow::Result<()>>,
    },
    InsertFinding {
        id: String,
        cycle_id: String,
        severity: String,
        dimension: String,
        title: String,
        description: Option<String>,
        location: Option<String>,
        fix: Option<String>,
        resp: oneshot::Sender<anyhow::Result<()>>,
    },
    InsertLearning {
        id: String,
        learning_type: String,
        content: String,
        category: Option<String>,
        confidence: f64,
        source_task: Option<String>,
        resp: oneshot::Sender<anyhow::Result<()>>,
    },
    RunDecay {
        rate: f32,
        resp: oneshot::Sender<anyhow::Result<usize>>,
    },
}

/// A handle to the Store that uses message passing.
#[derive(Clone)]
pub struct StoreHandle {
    tx: mpsc::Sender<StoreCommand>,
}

impl StoreHandle {
    pub fn new(tx: mpsc::Sender<StoreCommand>) -> Self {
        Self { tx }
    }

    pub async fn insert_session(
        &self,
        id: String,
        channel: String,
        model_provider: String,
        model_id: String,
    ) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::InsertSession {
                id,
                channel,
                model_provider,
                model_id,
                resp: resp_tx,
            })
            .await?;
        resp_rx.await?
    }

    pub async fn insert_task(
        &self,
        id: String,
        description: String,
        category: Option<String>,
        session_id: Option<String>,
    ) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::InsertTask {
                id,
                description,
                category,
                session_id,
                resp: resp_tx,
            })
            .await?;
        resp_rx.await?
    }

    pub async fn query_events_since(&self, since: &str) -> anyhow::Result<Vec<UsageEventRow>> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::QueryEventsSince {
                since: since.to_string(),
                resp: resp_tx,
            })
            .await?;
        resp_rx.await?
    }
    pub async fn count_learnings(&self) -> anyhow::Result<usize> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::CountLearnings { resp: resp_tx })
            .await?;
        resp_rx.await?
    }

    pub async fn query_all_learnings(&self) -> anyhow::Result<Vec<LearningRow>> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::QueryAllLearnings { resp: resp_tx })
            .await?;
        resp_rx.await?
    }

    pub async fn reinforce_learning(&self, id: String) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::ReinforceLearning { id, resp: resp_tx })
            .await?;
        resp_rx.await?
    }

    pub async fn query_approved_patterns(&self) -> anyhow::Result<Vec<UsagePatternRow>> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::QueryApprovedPatterns { resp: resp_tx })
            .await?;
        resp_rx.await?
    }

    pub async fn query_skill_effectiveness(
        &self,
        skill_name: String,
        task_category: String,
    ) -> anyhow::Result<Option<crate::memory::store::SkillEffectivenessRow>> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::QuerySkillEffectiveness {
                skill_name,
                task_category,
                resp: resp_tx,
            })
            .await?;
        resp_rx.await?
    }

    pub async fn query_learnings_by_type(
        &self,
        learning_type: String,
        limit: u32,
    ) -> anyhow::Result<Vec<LearningRow>> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::QueryLearningsByType {
                learning_type,
                limit,
                resp: resp_tx,
            })
            .await?;
        resp_rx.await?
    }

    pub async fn query_top_skills_for_category(
        &self,
        category: String,
        limit: u32,
    ) -> anyhow::Result<Vec<crate::memory::store::SkillEffectivenessRow>> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::QueryTopSkillsForCategory {
                category,
                limit,
                resp: resp_tx,
            })
            .await?;
        resp_rx.await?
    }

    pub async fn update_session_totals(
        &self,
        id: String,
        tokens: i64,
        cost: f64,
    ) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::UpdateSessionTotals {
                id,
                tokens,
                cost,
                resp: resp_tx,
            })
            .await?;
        resp_rx.await?
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn complete_task(
        &self,
        id: String,
        final_score: f64,
        iterations: i32,
        decision: String,
        total_tokens: i64,
        total_cost: f64,
    ) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::CompleteTask {
                id,
                final_score,
                iterations,
                decision,
                total_tokens,
                total_cost,
                resp: resp_tx,
            })
            .await?;
        resp_rx.await?
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_cycle(
        &self,
        id: String,
        task_id: String,
        iteration: i32,
        score: Option<f64>,
        decision: String,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        duration_ms: Option<i64>,
    ) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::InsertCycle {
                id,
                task_id,
                iteration,
                score,
                decision,
                input_tokens,
                output_tokens,
                duration_ms,
                resp: resp_tx,
            })
            .await?;
        resp_rx.await?
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_finding(
        &self,
        id: String,
        cycle_id: String,
        severity: String,
        dimension: String,
        title: String,
        description: Option<String>,
        location: Option<String>,
        fix: Option<String>,
    ) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::InsertFinding {
                id,
                cycle_id,
                severity,
                dimension,
                title,
                description,
                location,
                fix,
                resp: resp_tx,
            })
            .await?;
        resp_rx.await?
    }

    pub async fn insert_learning(
        &self,
        id: String,
        learning_type: String,
        content: String,
        category: Option<String>,
        confidence: f64,
        source_task: Option<String>,
    ) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::InsertLearning {
                id,
                learning_type,
                content,
                category,
                confidence,
                source_task,
                resp: resp_tx,
            })
            .await?;
        resp_rx.await?
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_usage_event(
        &self,
        id: String,
        event_type: String,
        channel: Option<String>,
        description: Option<String>,
        category: Option<String>,
        skills_used: Option<String>,
        score: Option<f64>,
        day: String,
        hour: Option<i32>,
        day_of_week: Option<i32>,
    ) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::InsertUsageEvent {
                id,
                event_type,
                channel,
                description,
                category,
                skills_used,
                score,
                day,
                hour,
                day_of_week,
                resp: resp_tx,
            })
            .await?;
        resp_rx.await?
    }

    pub async fn run_decay(&self, rate: f32) -> anyhow::Result<usize> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(StoreCommand::RunDecay {
                rate,
                resp: resp_tx,
            })
            .await?;
        resp_rx.await?
    }
}

/// Helper to spawn the store server and return a handle.
pub fn spawn_store_server(store: Store) -> (StoreHandle, tokio::task::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(100);
    let handle = StoreHandle::new(tx);
    let join_handle = tokio::spawn(run_store_server(store, rx));
    (handle, join_handle)
}

/// The background task that owns the Store.
pub async fn run_store_server(store: Store, mut rx: mpsc::Receiver<StoreCommand>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            StoreCommand::InsertSession {
                id,
                channel,
                model_provider,
                model_id,
                resp,
            } => {
                let res = store.insert_session(&id, &channel, &model_provider, &model_id);
                let _ = resp.send(res);
            }
            StoreCommand::UpdateSessionTotals {
                id,
                tokens,
                cost,
                resp,
            } => {
                let res = store.update_session_totals(&id, tokens, cost);
                let _ = resp.send(res);
            }
            StoreCommand::InsertTask {
                id,
                description,
                category,
                session_id,
                resp,
            } => {
                let res = store.insert_task(
                    &id,
                    &description,
                    category.as_deref(),
                    session_id.as_deref(),
                );
                let _ = resp.send(res);
            }
            StoreCommand::CompleteTask {
                id,
                final_score,
                iterations,
                decision,
                total_tokens,
                total_cost,
                resp,
            } => {
                let res = store.complete_task(
                    &id,
                    final_score,
                    iterations,
                    &decision,
                    total_tokens,
                    total_cost,
                );
                let _ = resp.send(res);
            }
            StoreCommand::InsertUsageEvent {
                id,
                event_type,
                channel,
                description,
                category,
                skills_used,
                score,
                day,
                hour,
                day_of_week,
                resp,
            } => {
                let res = store.insert_usage_event(
                    &id,
                    &event_type,
                    channel.as_deref(),
                    description.as_deref(),
                    category.as_deref(),
                    skills_used.as_deref(),
                    score,
                    &day,
                    hour,
                    day_of_week,
                );
                let _ = resp.send(res);
            }
            StoreCommand::QueryEventsSince { since, resp } => {
                let res = store.query_events_since(&since);
                let _ = resp.send(res);
            }
            StoreCommand::QueryApprovedPatterns { resp } => {
                let res = store.query_approved_patterns();
                let _ = resp.send(res);
            }
            StoreCommand::QuerySkillEffectiveness {
                skill_name,
                task_category,
                resp,
            } => {
                let res = store.query_skill_effectiveness(&skill_name, &task_category);
                let _ = resp.send(res);
            }
            StoreCommand::QueryLearningsByType {
                learning_type,
                limit,
                resp,
            } => {
                let res = store.query_learnings_by_type(&learning_type, limit);
                let _ = resp.send(res);
            }
            StoreCommand::QueryTopSkillsForCategory {
                category,
                limit,
                resp,
            } => {
                let res = store.query_top_skills_for_category(&category, limit);
                let _ = resp.send(res);
            }
            StoreCommand::CountLearnings { resp } => {
                let res = store.count_learnings().map(|c| c as usize);
                let _ = resp.send(res);
            }
            StoreCommand::QueryAllLearnings { resp } => {
                let res = store.query_all_learnings();
                let _ = resp.send(res);
            }
            StoreCommand::ReinforceLearning { id, resp } => {
                let res = store.reinforce_learning(&id);
                let _ = resp.send(res);
            }
            StoreCommand::InsertCycle {
                id,
                task_id,
                iteration,
                score,
                decision,
                input_tokens,
                output_tokens,
                duration_ms,
                resp,
            } => {
                let res = store.insert_cycle(
                    &id,
                    &task_id,
                    iteration,
                    score,
                    &decision,
                    input_tokens,
                    output_tokens,
                    duration_ms,
                );
                let _ = resp.send(res);
            }
            StoreCommand::InsertFinding {
                id,
                cycle_id,
                severity,
                dimension,
                title,
                description,
                location,
                fix,
                resp,
            } => {
                let res = store.insert_finding(
                    &id,
                    &cycle_id,
                    &severity,
                    &dimension,
                    &title,
                    description.as_deref(),
                    location.as_deref(),
                    fix.as_deref(),
                );
                let _ = resp.send(res);
            }
            StoreCommand::InsertLearning {
                id,
                learning_type,
                content,
                category,
                confidence,
                source_task,
                resp,
            } => {
                let res = store.insert_learning(
                    &id,
                    &learning_type,
                    &content,
                    category.as_deref(),
                    confidence,
                    source_task.as_deref(),
                );
                let _ = resp.send(res);
            }
            StoreCommand::RunDecay { rate, resp } => {
                let res = crate::memory::decay::run_decay(&store, rate);
                let _ = resp.send(res);
            }
        }
    }
}
