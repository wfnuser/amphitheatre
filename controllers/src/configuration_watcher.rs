// Copyright 2023 The Amphitheatre Authors.
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

use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::ConfigMap;
use kube::api::ListParams;
use kube::runtime::{watcher, WatchStreamExt};
use kube::Api;
use tracing::{debug, error};

use crate::context::Context;
use crate::error::Result;

pub async fn new(ctx: &Arc<Context>) {
    let api = Api::<ConfigMap>::namespaced(ctx.k8s.clone(), "amp-system");

    let params = ListParams::default().fields("metadata.name=amp-configurations");
    let mut obs = watcher(api, params).applied_objects().boxed();

    loop {
        let config_map = obs.try_next().await;

        match config_map {
            Ok(Some(cm)) => {
                if let Err(err) = handle_config_map(ctx, &cm) {
                    error!("Handle config map failed: {}", err.to_string());
                }
            }
            Ok(None) => continue,
            Err(err) => {
                error!("Resolve config config stream failed: {}", err.to_string());
                continue;
            }
        }
    }
}

// This function lets the app handle an added/modified configmap from k8s.
fn handle_config_map(_ctx: &Arc<Context>, cm: &ConfigMap) -> Result<()> {
    debug!("Handle an added/modified configmap from k8s: {:#?}", cm.data);
    Ok(())
}
