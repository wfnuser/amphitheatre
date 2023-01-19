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

use std::sync::Arc;

use futures::{future, StreamExt};
use kube::api::ListParams;
use kube::runtime::Controller;
use kube::{Api, Client};

use crate::app::Context;
use crate::resources::crds::{Actor, Playbook};

pub mod actor_controller;
pub mod playbook_controller;

pub struct Ctx {
    pub client: Client,
}

/// Initialize the controller and shared state (given the crd is installed)
pub async fn run(ctx: Arc<Context>) {
    let playbook = Api::<Playbook>::all(ctx.k8s.clone());
    let actor = Api::<Actor>::all(ctx.k8s.clone());

    // Ensure CRD is installed before loop-watching
    if let Err(e) = playbook.list(&ListParams::default().limit(1)).await {
        tracing::error!("Playbook CRD is not queryable; {e:?}. Is the CRD installed?");
        tracing::info!("Installation: cargo run --bin crdgen | kubectl apply -f -");
        std::process::exit(1);
    }

    if let Err(e) = actor.list(&ListParams::default().limit(1)).await {
        tracing::error!("Actor CRD is not queryable; {e:?}. Is the CRD installed?");
        tracing::info!("Installation: cargo run --bin crdgen | kubectl apply -f -");
        std::process::exit(1);
    }

    let context = Arc::new(Ctx {
        client: ctx.k8s.clone(),
    });

    let playbook_ctrl = Controller::new(playbook, ListParams::default())
        .run(
            playbook_controller::reconcile,
            playbook_controller::error_policy,
            context.clone(),
        )
        .for_each(|_| future::ready(()));

    let actor_ctrl = Controller::new(actor, ListParams::default())
        .run(
            actor_controller::reconcile,
            actor_controller::error_policy,
            context.clone(),
        )
        .for_each(|_| future::ready(()));

    tokio::select! {
        _ = playbook_ctrl => tracing::warn!("playbook controller exited"),
        _ = actor_ctrl => tracing::warn!("actor controller exited"),
    }
}
