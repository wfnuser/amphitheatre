// Copyright 2022 The Amphitheatre Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use kube::runtime::controller::Action;
use kube::runtime::events::{Event, EventType, Recorder};
use kube::runtime::finalizer::{finalizer, Event as FinalizerEvent};
use kube::{Api, Client, Resource, ResourceExt};

use crate::resources::error::{Error, Result};
use crate::resources::types::{Actor, Playbook, PlaybookState, PLAYBOOK_RESOURCE_NAME};
use crate::resources::{actor, playbook};

pub struct Ctx {
    /// Kubernetes client
    pub client: Client,
}

impl Ctx {
    fn recorder(&self, playbook: &Playbook) -> Recorder {
        Recorder::new(
            self.client.clone(),
            "amphitheatre-composer".into(),
            playbook.object_ref(&()),
        )
    }
}

/// The reconciler that will be called when either object change
pub async fn reconcile(playbook: Arc<Playbook>, ctx: Arc<Ctx>) -> Result<Action> {
    let ns = playbook.namespace().unwrap(); // doc is namespace scoped
    let api: Api<Playbook> = Api::namespaced(ctx.client.clone(), &ns);

    tracing::info!("Reconciling Playbook \"{}\" in {}", playbook.name_any(), ns);
    if playbook.spec.actors.is_empty() {
        return Err(Error::EmptyActorsError);
    }

    finalizer(&api, PLAYBOOK_RESOURCE_NAME, playbook, |event| async {
        match event {
            FinalizerEvent::Apply(playbook) => playbook.reconcile(ctx.clone()).await,
            FinalizerEvent::Cleanup(playbook) => playbook.cleanup(ctx.clone()).await,
        }
    })
    .await
    .map_err(|e| Error::FinalizerError(Box::new(e)))
}
/// an error handler that will be called when the reconciler fails with access to both the
/// object that caused the failure and the actual error
pub fn error_policy(playbook: Arc<Playbook>, error: &Error, ctx: Arc<Ctx>) -> Action {
    tracing::error!("reconcile failed: {:?}", error);
    Action::requeue(Duration::from_secs(60))
}

impl Playbook {
    pub async fn reconcile(&self, ctx: Arc<Ctx>) -> Result<Action> {
        if let Some(ref status) = self.status {
            if status.pending() {
                self.start(ctx).await?
            } else if status.solving() {
                self.solve(ctx).await?
            } else if status.running() {
                self.run(ctx).await?
            }
        } else {
            tracing::debug!("Waiting for PlaybookStatus to be reported, not starting yet");
        }

        Ok(Action::await_change())
    }

    async fn start(&self, ctx: Arc<Ctx>) -> Result<()> {
        playbook::patch_status(ctx.client.clone(), self, PlaybookState::solving()).await
    }

    async fn solve(&self, ctx: Arc<Ctx>) -> Result<()> {
        let exists: HashSet<String> = self.spec.actors.iter().map(|a| a.repo.clone()).collect();
        let mut fetches: HashSet<String> = HashSet::new();

        for actor in &self.spec.actors {
            if actor.partners.is_empty() {
                continue;
            }

            for repo in &actor.partners {
                if exists.contains(repo) {
                    continue;
                }
                fetches.insert(repo.to_string());
            }
        }

        for url in fetches.iter() {
            tracing::info!("fetches url: {}", url);
            let actor: Actor = read_partner(url);
            actor::add(ctx.client.clone(), self, actor).await?;
        }

        tracing::info!("fetches length: {}", fetches.len());

        if fetches.is_empty() {
            playbook::patch_status(ctx.client.clone(), self, PlaybookState::ready()).await?;
        }

        Ok(())
    }

    async fn run(&self, ctx: Arc<Ctx>) -> Result<()> {
        for actor in &self.spec.actors {
            actor::build(ctx.client.clone(), self, actor).await?;
            actor::deploy(ctx.client.clone(), self, actor).await?;
        }
        Ok(())
    }

    pub async fn cleanup(&self, ctx: Arc<Ctx>) -> Result<Action> {
        // todo add some deletion event logging, db clean up, etc.?
        let recorder = ctx.recorder(self);
        // Doesn't have dependencies in this example case, so we just publish an event
        recorder
            .publish(Event {
                type_: EventType::Normal,
                reason: "DeletePlaybook".into(),
                note: Some(format!("Delete playbook `{}`", self.name_any())),
                action: "Reconciling".into(),
                secondary: None,
            })
            .await
            .map_err(Error::KubeError)?;
        Ok(Action::await_change())
    }
}

fn read_partner(url: &String) -> Actor {
    Actor {
        name: "amp-example-nodejs".into(),
        description: "A simple NodeJs example app".into(),
        image: "amp-example-nodejs".into(),
        repo: url.into(),
        path: ".".into(),
        reference: "master".into(),
        commit: "285ef2bc98fb6b3db46a96b6a750fad2d0c566b5".into(),
        environment: HashMap::new(),
        partners: vec![],
    }
}
