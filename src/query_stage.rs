// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

use crate::context::serialize_execution_plan;
use crate::shuffle::{ShuffleCodec, ShuffleReaderExec};
use datafusion::error::Result;
use datafusion::physical_plan::{ExecutionPlan, ExecutionPlanProperties, Partitioning};
use datafusion::prelude::SessionContext;
use datafusion_proto::bytes::physical_plan_from_bytes_with_extension_codec;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use std::sync::Arc;

#[pyclass(name = "QueryStage", module = "datafusion_ray", subclass)]
pub struct PyQueryStage {
    stage: Arc<QueryStage>,
}

impl PyQueryStage {
    pub fn from_rust(stage: Arc<QueryStage>) -> Self {
        Self { stage }
    }
}

#[pymethods]
impl PyQueryStage {
    #[new]
    pub fn new(id: usize, bytes: Vec<u8>) -> Result<Self> {
        let ctx = SessionContext::new();
        let codec = ShuffleCodec {};
        let plan = physical_plan_from_bytes_with_extension_codec(&bytes, &ctx, &codec)?;
        Ok(PyQueryStage {
            stage: Arc::new(QueryStage { id, plan }),
        })
    }

    pub fn id(&self) -> usize {
        self.stage.id
    }

    pub fn get_execution_plan_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        serialize_execution_plan(self.stage.plan.clone(), py)
    }

    pub fn get_child_stage_ids(&self) -> Vec<usize> {
        self.stage.get_child_stage_ids()
    }

    pub fn get_input_partition_count(&self) -> usize {
        self.stage.get_input_partition_count()
    }

    pub fn get_output_partition_count(&self) -> usize {
        self.stage.get_output_partition_count()
    }
}

#[derive(Debug)]
pub struct QueryStage {
    pub id: usize,
    pub plan: Arc<dyn ExecutionPlan>,
}

fn _get_output_partition_count(plan: &dyn ExecutionPlan) -> usize {
    // UnknownPartitioning and HashPartitioning with empty expressions will
    // both return 1 partition.
    match plan.properties().output_partitioning() {
        Partitioning::UnknownPartitioning(_) => 1,
        Partitioning::Hash(expr, _) if expr.is_empty() => 1,
        p => p.partition_count(),
    }
}

impl QueryStage {
    pub fn new(id: usize, plan: Arc<dyn ExecutionPlan>) -> Self {
        Self { id, plan }
    }

    pub fn get_child_stage_ids(&self) -> Vec<usize> {
        let mut ids = vec![];
        collect_child_stage_ids(self.plan.as_ref(), &mut ids);
        ids
    }

    /// Get the input partition count. This is the same as the number of concurrent tasks
    /// when we schedule this query stage for execution
    pub fn get_input_partition_count(&self) -> usize {
        if self.plan.children().is_empty() {
            // leaf node (file scan)
            self.plan.output_partitioning().partition_count()
        } else {
            self.plan.children()[0]
                .output_partitioning()
                .partition_count()
        }
    }

    pub fn get_output_partition_count(&self) -> usize {
        _get_output_partition_count(self.plan.as_ref())
    }
}

fn collect_child_stage_ids(plan: &dyn ExecutionPlan, ids: &mut Vec<usize>) {
    if let Some(shuffle_reader) = plan.as_any().downcast_ref::<ShuffleReaderExec>() {
        ids.push(shuffle_reader.stage_id);
    } else {
        for child_plan in plan.children() {
            collect_child_stage_ids(child_plan.as_ref(), ids);
        }
    }
}
