use super::*;

/// ADR-0120 decision 7's Board Doctor: "may name missing task publication,
/// stale projection, impossible state/lease combinations, unresolved waits,
/// and scope overlap, but it never mutates a task."
#[derive(Debug, Clone, PartialEq)]
pub enum WorkDoctorFinding {
    /// A live claim (ClaimStore) has no corresponding Work task at all.
    ClaimsOnly {
        task_id: String,
        owner_id: String,
        lease_expires_at: SystemTime,
    },
    /// Two non-terminal tasks under the same goal share an exact title.
    DuplicateTitleSameGoal {
        task_id: String,
        duplicate_of_task_id: String,
        title: String,
        goal_id: String,
    },
    /// A terminal task still carries an owner or a live lease, or a
    /// non-open/claimed task has no owner despite an active lease.
    ImpossibleStateLeaseCombination {
        task_id: String,
        state: WorkTaskState,
        detail: String,
    },
    /// A wait has stood unanswered longer than the staleness threshold on a
    /// still-open task.
    UnansweredWait {
        task_id: String,
        wait_id: String,
        question: String,
        asked_at: SystemTime,
    },
    /// Two non-terminal tasks in the same repository declare an overlapping
    /// path.
    DeclaredScopeOverlap {
        task_id: String,
        overlaps_with_task_id: String,
        path: String,
    },
}

impl WorkStore {
    /// ADR-0120 decision 7's Board Doctor findings for one repository. Pure
    /// diagnosis: it reads `work_tasks` and `delegated_claims` and never
    /// writes.
    pub async fn board_doctor(
        &self,
        tenant_id: &str,
        repository_id: &str,
        now: SystemTime,
        unanswered_wait_threshold: std::time::Duration,
    ) -> Result<Vec<WorkDoctorFinding>, WorkStoreError> {
        let mut findings = Vec::new();

        let (_, claims_only) = self
            .claims_only_records(tenant_id, repository_id, now, i64::MAX)
            .await?;
        for claim in claims_only {
            findings.push(WorkDoctorFinding::ClaimsOnly {
                task_id: claim.task_id,
                owner_id: claim.owner_id,
                lease_expires_at: claim.lease_expires_at,
            });
        }

        let task_rows = self
            .client
            .query(
                "SELECT task_id, title, goal_id, state, owner_id, lease_expires_at, declared_paths \
                 FROM work_tasks WHERE tenant_id = $1 AND repository_id = $2 ORDER BY task_id",
                &[&tenant_id, &repository_id],
            )
            .await?;
        struct Task {
            task_id: String,
            title: String,
            goal_id: Option<String>,
            state: WorkTaskState,
            owner_id: Option<String>,
            lease_expires_at: Option<SystemTime>,
            declared_paths: Vec<String>,
        }
        let mut tasks = Vec::with_capacity(task_rows.len());
        for row in &task_rows {
            tasks.push(Task {
                task_id: row.get("task_id"),
                title: row.get("title"),
                goal_id: row.get("goal_id"),
                state: WorkTaskState::from_i16(row.get("state"))?,
                owner_id: row.get("owner_id"),
                lease_expires_at: row.get("lease_expires_at"),
                declared_paths: row.get("declared_paths"),
            });
        }

        for (index, task) in tasks.iter().enumerate() {
            if task.state.is_terminal() {
                if task.owner_id.is_some() || task.lease_expires_at.is_some() {
                    findings.push(WorkDoctorFinding::ImpossibleStateLeaseCombination {
                        task_id: task.task_id.clone(),
                        state: task.state,
                        detail: "a terminal task still carries an owner or a live lease".to_owned(),
                    });
                }
                continue;
            }
            if !matches!(task.state, WorkTaskState::Claimed) && task.owner_id.is_some() {
                findings.push(WorkDoctorFinding::ImpossibleStateLeaseCombination {
                    task_id: task.task_id.clone(),
                    state: task.state,
                    detail: "an owner is set on a task that is not claimed".to_owned(),
                });
            }

            if let Some(goal_id) = &task.goal_id {
                for other in tasks.iter().skip(index + 1) {
                    if other.state.is_terminal() {
                        continue;
                    }
                    if other.goal_id.as_deref() == Some(goal_id.as_str())
                        && other.title == task.title
                    {
                        findings.push(WorkDoctorFinding::DuplicateTitleSameGoal {
                            task_id: other.task_id.clone(),
                            duplicate_of_task_id: task.task_id.clone(),
                            title: task.title.clone(),
                            goal_id: goal_id.clone(),
                        });
                    }
                }
            }

            for other in tasks.iter().skip(index + 1) {
                if other.state.is_terminal() {
                    continue;
                }
                for path in &task.declared_paths {
                    if other.declared_paths.contains(path) {
                        findings.push(WorkDoctorFinding::DeclaredScopeOverlap {
                            task_id: other.task_id.clone(),
                            overlaps_with_task_id: task.task_id.clone(),
                            path: path.clone(),
                        });
                    }
                }
            }
        }

        let stale_before = now
            .checked_sub(unanswered_wait_threshold)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let wait_rows = self
            .client
            .query(
                "SELECT w.wait_id, w.task_id, w.question, w.asked_at FROM work_task_waits w \
                 INNER JOIN work_tasks t \
                    ON t.tenant_id = w.tenant_id AND t.repository_id = w.repository_id \
                   AND t.task_id = w.task_id \
                 WHERE w.tenant_id = $1 AND w.repository_id = $2 AND w.answered_at IS NULL \
                   AND w.asked_at < $3 AND t.state NOT IN (7, 8) \
                 ORDER BY w.asked_at ASC",
                &[&tenant_id, &repository_id, &stale_before],
            )
            .await?;
        for row in &wait_rows {
            findings.push(WorkDoctorFinding::UnansweredWait {
                task_id: row.get("task_id"),
                wait_id: row.get("wait_id"),
                question: row.get("question"),
                asked_at: row.get("asked_at"),
            });
        }

        Ok(findings)
    }

    /// The `UnansweredWait` finding from `board_doctor`, read across ALL of a
    /// tenant's repositories at once and bounded to `limit` oldest-first --
    /// the cross-repository view the Bridge Agents page needs so an operator
    /// does not have to open Board Doctor once per repository to find a
    /// stalled agent.
    pub async fn fleet_unanswered_waits(
        &self,
        tenant_id: &str,
        now: SystemTime,
        unanswered_wait_threshold: std::time::Duration,
        limit: i64,
    ) -> Result<Vec<FleetUnansweredWait>, WorkStoreError> {
        let stale_before = now
            .checked_sub(unanswered_wait_threshold)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let rows = self
            .client
            .query(
                "SELECT w.repository_id, w.task_id, w.wait_id, w.question, w.asked_at \
                 FROM work_task_waits w \
                 INNER JOIN work_tasks t \
                    ON t.tenant_id = w.tenant_id AND t.repository_id = w.repository_id \
                   AND t.task_id = w.task_id \
                 WHERE w.tenant_id = $1 AND w.answered_at IS NULL \
                   AND w.asked_at < $2 AND t.state NOT IN (7, 8) \
                 ORDER BY w.asked_at ASC LIMIT $3",
                &[&tenant_id, &stale_before, &limit],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| FleetUnansweredWait {
                repository_id: row.get("repository_id"),
                task_id: row.get("task_id"),
                wait_id: row.get("wait_id"),
                question: row.get("question"),
                asked_at: row.get("asked_at"),
            })
            .collect())
    }
}

/// One unresolved wait, named to its repository, as read by
/// `fleet_unanswered_waits` -- the cross-repository counterpart of
/// `WorkDoctorFinding::UnansweredWait`.
#[derive(Debug, Clone, PartialEq)]
pub struct FleetUnansweredWait {
    pub repository_id: String,
    pub task_id: String,
    pub wait_id: String,
    pub question: String,
    pub asked_at: SystemTime,
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::test_support::unique_id;

    async fn raw_client(database_url: &str) -> Client {
        let (client, connection) = tokio_postgres::connect(database_url, tokio_postgres::NoTls)
            .await
            .expect("connect raw test client");
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client
    }

    fn truncate_to_micros(time: SystemTime) -> SystemTime {
        let since_epoch = time
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time is after the Unix epoch");
        SystemTime::UNIX_EPOCH + Duration::from_micros(since_epoch.as_micros() as u64)
    }

    fn new_task(tenant_id: &str, repository_id: &str, task_id: &str, title: &str) -> NewWorkTask {
        NewWorkTask {
            tenant_id: tenant_id.to_owned(),
            repository_id: repository_id.to_owned(),
            task_id: task_id.to_owned(),
            title: title.to_owned(),
            acceptance: "acceptance text".to_owned(),
            goal_id: None,
            declared_paths: Vec::new(),
            declared_symbols: Vec::new(),
            published_by: "test-actor".to_owned(),
        }
    }

    #[tokio::test]
    async fn board_doctor_is_clean_for_one_well_formed_open_task() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let mut store = WorkStore::connect(&database_url).await.expect("connect");
        store
            .create_task(
                &new_task(
                    &tenant_id,
                    &repository_id,
                    &unique_id("task"),
                    "Ship the thing",
                ),
                &unique_id("event"),
                SystemTime::now(),
            )
            .await
            .expect("create task");

        let findings = store
            .board_doctor(
                &tenant_id,
                &repository_id,
                SystemTime::now(),
                Duration::from_secs(3600),
            )
            .await
            .expect("board doctor");

        assert_eq!(findings, Vec::new());
    }

    #[tokio::test]
    async fn board_doctor_finds_a_claim_with_no_work_task() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let task_id = unique_id("task");
        let store = WorkStore::connect(&database_url).await.expect("connect");
        let raw = raw_client(&database_url).await;
        let now = SystemTime::now();
        let lease_expires_at = truncate_to_micros(now + Duration::from_secs(600));
        raw.execute(
            "INSERT INTO delegated_claims (tenant_id, repository_id, task_id, owner_id, branch, \
                claim_started_at, lease_expires_at, claim_lapses, paths, symbols) \
             VALUES ($1,$2,$3,'owner-1','main',$4,$5,0,'{}','{}')",
            &[
                &tenant_id,
                &repository_id,
                &task_id,
                &now,
                &lease_expires_at,
            ],
        )
        .await
        .expect("insert delegated claim");

        let findings = store
            .board_doctor(&tenant_id, &repository_id, now, Duration::from_secs(3600))
            .await
            .expect("board doctor");

        assert_eq!(
            findings,
            vec![WorkDoctorFinding::ClaimsOnly {
                task_id,
                owner_id: "owner-1".to_owned(),
                lease_expires_at,
            }]
        );
    }

    #[tokio::test]
    async fn board_doctor_finds_two_open_tasks_sharing_a_title_under_the_same_goal() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let goal_id = unique_id("goal");
        let first_task_id = unique_id("task");
        let second_task_id = unique_id("task");
        let mut store = WorkStore::connect(&database_url).await.expect("connect");
        let mut first = new_task(
            &tenant_id,
            &repository_id,
            &first_task_id,
            "Duplicate title",
        );
        first.goal_id = Some(goal_id.clone());
        let mut second = new_task(
            &tenant_id,
            &repository_id,
            &second_task_id,
            "Duplicate title",
        );
        second.goal_id = Some(goal_id.clone());
        store
            .create_task(&first, &unique_id("event"), SystemTime::now())
            .await
            .expect("create first task");
        store
            .create_task(&second, &unique_id("event"), SystemTime::now())
            .await
            .expect("create second task");

        let findings = store
            .board_doctor(
                &tenant_id,
                &repository_id,
                SystemTime::now(),
                Duration::from_secs(3600),
            )
            .await
            .expect("board doctor");

        assert_eq!(
            findings,
            vec![WorkDoctorFinding::DuplicateTitleSameGoal {
                task_id: second_task_id,
                duplicate_of_task_id: first_task_id,
                title: "Duplicate title".to_owned(),
                goal_id,
            }]
        );
    }

    #[tokio::test]
    async fn board_doctor_ignores_a_shared_title_when_the_goal_differs() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let mut store = WorkStore::connect(&database_url).await.expect("connect");
        let mut first = new_task(&tenant_id, &repository_id, &unique_id("task"), "Same title");
        first.goal_id = Some(unique_id("goal"));
        let mut second = new_task(&tenant_id, &repository_id, &unique_id("task"), "Same title");
        second.goal_id = Some(unique_id("goal"));
        store
            .create_task(&first, &unique_id("event"), SystemTime::now())
            .await
            .expect("create first task");
        store
            .create_task(&second, &unique_id("event"), SystemTime::now())
            .await
            .expect("create second task");

        let findings = store
            .board_doctor(
                &tenant_id,
                &repository_id,
                SystemTime::now(),
                Duration::from_secs(3600),
            )
            .await
            .expect("board doctor");

        assert_eq!(findings, Vec::new());
    }

    #[tokio::test]
    async fn board_doctor_finds_an_owner_set_on_a_task_that_is_not_claimed() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let task_id = unique_id("task");
        let mut store = WorkStore::connect(&database_url).await.expect("connect");
        store
            .create_task(
                &new_task(&tenant_id, &repository_id, &task_id, "Ship the thing"),
                &unique_id("event"),
                SystemTime::now(),
            )
            .await
            .expect("create task");
        let raw = raw_client(&database_url).await;
        let lease_expires_at = SystemTime::now() + Duration::from_secs(600);
        raw.execute(
            "UPDATE work_tasks SET owner_id = 'owner-1', lease_expires_at = $4 \
             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
            &[&tenant_id, &repository_id, &task_id, &lease_expires_at],
        )
        .await
        .expect("sabotage: set owner on an open task");

        let findings = store
            .board_doctor(
                &tenant_id,
                &repository_id,
                SystemTime::now(),
                Duration::from_secs(3600),
            )
            .await
            .expect("board doctor");

        assert_eq!(
            findings,
            vec![WorkDoctorFinding::ImpossibleStateLeaseCombination {
                task_id,
                state: WorkTaskState::Open,
                detail: "an owner is set on a task that is not claimed".to_owned(),
            }]
        );
    }

    #[tokio::test]
    async fn board_doctor_finds_a_long_unanswered_wait_on_a_still_open_task() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let task_id = unique_id("task");
        let wait_id = unique_id("wait");
        let mut store = WorkStore::connect(&database_url).await.expect("connect");
        store
            .create_task(
                &new_task(&tenant_id, &repository_id, &task_id, "Ship the thing"),
                &unique_id("event"),
                SystemTime::now(),
            )
            .await
            .expect("create task");
        let raw = raw_client(&database_url).await;
        let asked_at = truncate_to_micros(SystemTime::now() - Duration::from_secs(2 * 24 * 3600));
        raw.execute(
            "INSERT INTO work_task_waits (tenant_id, repository_id, wait_id, task_id, question, \
                asked_by, asked_at) VALUES ($1,$2,$3,$4,'is this still needed?','tester',$5)",
            &[&tenant_id, &repository_id, &wait_id, &task_id, &asked_at],
        )
        .await
        .expect("insert an old unanswered wait");

        let findings = store
            .board_doctor(
                &tenant_id,
                &repository_id,
                SystemTime::now(),
                Duration::from_secs(3600),
            )
            .await
            .expect("board doctor");

        assert_eq!(
            findings,
            vec![WorkDoctorFinding::UnansweredWait {
                task_id,
                wait_id,
                question: "is this still needed?".to_owned(),
                asked_at,
            }]
        );
    }

    #[tokio::test]
    async fn board_doctor_ignores_a_wait_still_inside_the_threshold() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let task_id = unique_id("task");
        let mut store = WorkStore::connect(&database_url).await.expect("connect");
        store
            .create_task(
                &new_task(&tenant_id, &repository_id, &task_id, "Ship the thing"),
                &unique_id("event"),
                SystemTime::now(),
            )
            .await
            .expect("create task");
        let raw = raw_client(&database_url).await;
        raw.execute(
            "INSERT INTO work_task_waits (tenant_id, repository_id, wait_id, task_id, question, \
                asked_by, asked_at) VALUES ($1,$2,$3,$4,'fresh question','tester',now())",
            &[&tenant_id, &repository_id, &unique_id("wait"), &task_id],
        )
        .await
        .expect("insert a fresh unanswered wait");

        let findings = store
            .board_doctor(
                &tenant_id,
                &repository_id,
                SystemTime::now(),
                Duration::from_secs(3600),
            )
            .await
            .expect("board doctor");

        assert_eq!(findings, Vec::new());
    }

    #[tokio::test]
    async fn board_doctor_finds_two_open_tasks_declaring_the_same_path() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let first_task_id = unique_id("task");
        let second_task_id = unique_id("task");
        let mut store = WorkStore::connect(&database_url).await.expect("connect");
        let mut first = new_task(&tenant_id, &repository_id, &first_task_id, "First task");
        first.declared_paths = vec!["src/shared.rs".to_owned()];
        let mut second = new_task(&tenant_id, &repository_id, &second_task_id, "Second task");
        second.declared_paths = vec!["src/shared.rs".to_owned()];
        store
            .create_task(&first, &unique_id("event"), SystemTime::now())
            .await
            .expect("create first task");
        store
            .create_task(&second, &unique_id("event"), SystemTime::now())
            .await
            .expect("create second task");

        let findings = store
            .board_doctor(
                &tenant_id,
                &repository_id,
                SystemTime::now(),
                Duration::from_secs(3600),
            )
            .await
            .expect("board doctor");

        assert_eq!(
            findings,
            vec![WorkDoctorFinding::DeclaredScopeOverlap {
                task_id: second_task_id,
                overlaps_with_task_id: first_task_id,
                path: "src/shared.rs".to_owned(),
            }]
        );
    }

    #[tokio::test]
    async fn board_doctor_ignores_a_terminal_tasks_owner_and_lease() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let task_id = unique_id("task");
        let mut store = WorkStore::connect(&database_url).await.expect("connect");
        store
            .create_task(
                &new_task(&tenant_id, &repository_id, &task_id, "Ship the thing"),
                &unique_id("event"),
                SystemTime::now(),
            )
            .await
            .expect("create task");
        let raw = raw_client(&database_url).await;
        raw.execute(
            "UPDATE work_tasks SET state = 7 WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
            &[&tenant_id, &repository_id, &task_id],
        )
        .await
        .expect("mark the task completed");

        let findings = store
            .board_doctor(
                &tenant_id,
                &repository_id,
                SystemTime::now(),
                Duration::from_secs(3600),
            )
            .await
            .expect("board doctor");

        assert_eq!(findings, Vec::new());
    }

    #[tokio::test]
    async fn board_doctor_finds_a_terminal_task_that_still_carries_an_owner() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let task_id = unique_id("task");
        let mut store = WorkStore::connect(&database_url).await.expect("connect");
        store
            .create_task(
                &new_task(&tenant_id, &repository_id, &task_id, "Ship the thing"),
                &unique_id("event"),
                SystemTime::now(),
            )
            .await
            .expect("create task");
        let raw = raw_client(&database_url).await;
        let lease_expires_at = SystemTime::now() + Duration::from_secs(600);
        raw.execute(
            "UPDATE work_tasks SET state = 7, owner_id = 'owner-1', lease_expires_at = $4 \
             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
            &[&tenant_id, &repository_id, &task_id, &lease_expires_at],
        )
        .await
        .expect("sabotage: mark the task completed but still leased");

        let findings = store
            .board_doctor(
                &tenant_id,
                &repository_id,
                SystemTime::now(),
                Duration::from_secs(3600),
            )
            .await
            .expect("board doctor");

        assert_eq!(
            findings,
            vec![WorkDoctorFinding::ImpossibleStateLeaseCombination {
                task_id,
                state: WorkTaskState::Completed,
                detail: "a terminal task still carries an owner or a live lease".to_owned(),
            }]
        );
    }

    #[tokio::test]
    async fn fleet_unanswered_waits_spans_every_repository_and_excludes_another_tenant() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let first_repository_id = unique_id("repo");
        let second_repository_id = unique_id("repo");
        let other_tenant_id = unique_id("tenant");
        let other_repository_id = unique_id("repo");
        let mut store = WorkStore::connect(&database_url).await.expect("connect");
        let raw = raw_client(&database_url).await;

        let older_task_id = unique_id("task");
        let older_wait_id = unique_id("wait");
        store
            .create_task(
                &new_task(
                    &tenant_id,
                    &first_repository_id,
                    &older_task_id,
                    "Ship the older thing",
                ),
                &unique_id("event"),
                SystemTime::now(),
            )
            .await
            .expect("create the first repository's task");
        let older_asked_at =
            truncate_to_micros(SystemTime::now() - Duration::from_secs(3 * 24 * 3600));
        raw.execute(
            "INSERT INTO work_task_waits (tenant_id, repository_id, wait_id, task_id, question, \
                asked_by, asked_at) VALUES ($1,$2,$3,$4,'older question','tester',$5)",
            &[
                &tenant_id,
                &first_repository_id,
                &older_wait_id,
                &older_task_id,
                &older_asked_at,
            ],
        )
        .await
        .expect("insert the older unanswered wait");

        let newer_task_id = unique_id("task");
        let newer_wait_id = unique_id("wait");
        store
            .create_task(
                &new_task(
                    &tenant_id,
                    &second_repository_id,
                    &newer_task_id,
                    "Ship the newer thing",
                ),
                &unique_id("event"),
                SystemTime::now(),
            )
            .await
            .expect("create the second repository's task");
        let newer_asked_at =
            truncate_to_micros(SystemTime::now() - Duration::from_secs(2 * 24 * 3600));
        raw.execute(
            "INSERT INTO work_task_waits (tenant_id, repository_id, wait_id, task_id, question, \
                asked_by, asked_at) VALUES ($1,$2,$3,$4,'newer question','tester',$5)",
            &[
                &tenant_id,
                &second_repository_id,
                &newer_wait_id,
                &newer_task_id,
                &newer_asked_at,
            ],
        )
        .await
        .expect("insert the newer unanswered wait");

        let other_task_id = unique_id("task");
        store
            .create_task(
                &new_task(
                    &other_tenant_id,
                    &other_repository_id,
                    &other_task_id,
                    "Ship another tenant's thing",
                ),
                &unique_id("event"),
                SystemTime::now(),
            )
            .await
            .expect("create the other tenant's task");
        let other_asked_at =
            truncate_to_micros(SystemTime::now() - Duration::from_secs(3 * 24 * 3600));
        raw.execute(
            "INSERT INTO work_task_waits (tenant_id, repository_id, wait_id, task_id, question, \
                asked_by, asked_at) VALUES ($1,$2,$3,$4,'other tenant question','tester',$5)",
            &[
                &other_tenant_id,
                &other_repository_id,
                &unique_id("wait"),
                &other_task_id,
                &other_asked_at,
            ],
        )
        .await
        .expect("insert the other tenant's unanswered wait");

        let waits = store
            .fleet_unanswered_waits(&tenant_id, SystemTime::now(), Duration::from_secs(3600), 20)
            .await
            .expect("fleet unanswered waits");

        assert_eq!(
            waits,
            vec![
                FleetUnansweredWait {
                    repository_id: first_repository_id,
                    task_id: older_task_id,
                    wait_id: older_wait_id,
                    question: "older question".to_owned(),
                    asked_at: older_asked_at,
                },
                FleetUnansweredWait {
                    repository_id: second_repository_id,
                    task_id: newer_task_id,
                    wait_id: newer_wait_id,
                    question: "newer question".to_owned(),
                    asked_at: newer_asked_at,
                },
            ]
        );
    }
}
