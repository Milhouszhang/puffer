use crate::spec::{
    AgentEnvNodeRef, AutomationFlowSpec, AutomationLoopInput, AutomationLoopSpec,
    AutomationLoopStopSpec, AutomationRecord, AutomationRuntimeState, AutomationSource,
    AutomationSpec, AutomationStepSpec, AutomationTriggerSpec, CompiledAgentEnvWorkflow,
    CompiledWorkflowRole, AUTOMATION_SPEC_VERSION,
};
use puffer_subscriptions::{normalize_contact_id, FilterSpec, TaggedFilterSpec};
use serde_json::Value;
use std::collections::BTreeSet;

/// Validates one persisted Automation record.
pub fn validate_automation_record(record: &AutomationRecord) -> Result<(), String> {
    validate_slug("automation.id", &record.id)?;
    validate_automation_spec(&record.spec)?;
    validate_automation_runtime_state(&record.runtime)?;
    validate_runtime_references_spec(&record.spec, &record.runtime)?;
    Ok(())
}

/// Validates the user-authored Automation spec.
pub fn validate_automation_spec(spec: &AutomationSpec) -> Result<(), String> {
    if spec.spec_version != AUTOMATION_SPEC_VERSION {
        return Err(format!(
            "automation spec_version must be {AUTOMATION_SPEC_VERSION}"
        ));
    }
    if spec.name.trim().is_empty() {
        return Err("automation.name must not be empty".into());
    }
    if spec.instructions.trim().is_empty() {
        return Err("automation.instructions must not be empty".into());
    }
    validate_automation_source(&spec.source)?;
    if spec.triggers.is_empty() {
        return Err("automation.triggers must contain at least one trigger".into());
    }
    validate_automation_triggers(&spec.triggers)?;
    if spec.flow.steps.is_empty() {
        return Err("automation.flow.steps must contain at least one step".into());
    }
    let mut step_ids = BTreeSet::new();
    validate_automation_flow(&spec.flow, &mut step_ids)?;
    if automation_flow_contains_loop(&spec.flow)
        && spec
            .triggers
            .iter()
            .any(|trigger| matches!(trigger, AutomationTriggerSpec::AgentEnvNode { .. }))
    {
        return Err(
            "loop automations currently support only puffer_connection or manual triggers".into(),
        );
    }
    Ok(())
}

/// Validates internal runtime compilation state for one Automation.
pub fn validate_automation_runtime_state(runtime: &AutomationRuntimeState) -> Result<(), String> {
    if let Some(hash) = &runtime.spec_hash {
        validate_hash_string("automation runtime spec_hash", hash)?;
    }
    for workflow in &runtime.agentenv_workflows {
        validate_compiled_agentenv_workflow(workflow)?;
    }
    for binding in &runtime.puffer_bindings {
        validate_local_id("compiled puffer binding trigger_id", &binding.trigger_id)?;
        validate_slug("compiled puffer binding slug", &binding.binding_slug)?;
    }
    Ok(())
}

fn validate_runtime_references_spec(
    spec: &AutomationSpec,
    runtime: &AutomationRuntimeState,
) -> Result<(), String> {
    let index = AutomationSpecIndex::from_spec(spec);

    for workflow in &runtime.agentenv_workflows {
        match &workflow.role {
            CompiledWorkflowRole::Root => {}
            CompiledWorkflowRole::LoopBody { step_id } => {
                if !index.loop_step_ids.contains(step_id) {
                    return Err(format!(
                        "compiled loop body step_id `{step_id}` must reference an automation loop step"
                    ));
                }
            }
            CompiledWorkflowRole::Continuation { step_id }
            | CompiledWorkflowRole::Helper { step_id } => {
                if !index.step_ids.contains(step_id) {
                    return Err(format!(
                        "compiled workflow role step_id `{step_id}` must reference an automation step"
                    ));
                }
            }
        }
    }

    for binding in &runtime.puffer_bindings {
        if !index
            .puffer_connection_trigger_ids
            .contains(&binding.trigger_id)
        {
            return Err(format!(
                "compiled puffer binding trigger_id `{}` must reference a puffer_connection trigger",
                binding.trigger_id
            ));
        }
    }

    Ok(())
}

struct AutomationSpecIndex {
    step_ids: BTreeSet<String>,
    loop_step_ids: BTreeSet<String>,
    puffer_connection_trigger_ids: BTreeSet<String>,
}

impl AutomationSpecIndex {
    fn from_spec(spec: &AutomationSpec) -> Self {
        let mut index = Self {
            step_ids: BTreeSet::new(),
            loop_step_ids: BTreeSet::new(),
            puffer_connection_trigger_ids: BTreeSet::new(),
        };

        for trigger in &spec.triggers {
            if let AutomationTriggerSpec::PufferConnection { id, .. } = trigger {
                index.puffer_connection_trigger_ids.insert(id.clone());
            }
        }
        index.add_flow(&spec.flow);
        index
    }

    fn add_flow(&mut self, flow: &AutomationFlowSpec) {
        for step in &flow.steps {
            match step {
                AutomationStepSpec::AgentEnvNode { id, .. } => {
                    self.step_ids.insert(id.clone());
                }
                AutomationStepSpec::Loop { id, body, .. } => {
                    self.step_ids.insert(id.clone());
                    self.loop_step_ids.insert(id.clone());
                    self.add_flow(body);
                }
            }
        }
    }
}

fn validate_automation_source(source: &AutomationSource) -> Result<(), String> {
    match source {
        AutomationSource::Blank => Ok(()),
        AutomationSource::NaturalLanguage { prompt } => {
            if prompt.trim().is_empty() {
                return Err("automation.source.prompt must not be empty".into());
            }
            Ok(())
        }
        AutomationSource::Template {
            template_id,
            template_version,
        } => {
            validate_local_id("automation.source.template_id", template_id)?;
            if template_version
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err("automation.source.template_version must not be empty".into());
            }
            Ok(())
        }
    }
}

fn validate_automation_triggers(triggers: &[AutomationTriggerSpec]) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    for trigger in triggers {
        let id = match trigger {
            AutomationTriggerSpec::AgentEnvNode { id, node, .. } => {
                validate_agentenv_node_ref(node)?;
                id
            }
            AutomationTriggerSpec::PufferConnection {
                id,
                connection_slug,
                connector_slug,
                filter,
                ignore_filters,
                contact_ids,
                ..
            } => {
                validate_slug("automation trigger connection_slug", connection_slug)?;
                if let Some(connector_slug) = connector_slug {
                    validate_slug("automation trigger connector_slug", connector_slug)?;
                }
                if let Some(filter) = filter {
                    validate_filter(filter)?;
                }
                for filter in ignore_filters {
                    validate_filter(filter)?;
                }
                validate_contact_ids(contact_ids)?;
                id
            }
            AutomationTriggerSpec::Manual { id, .. } => id,
        };
        validate_local_id("automation trigger id", id)?;
        if !ids.insert(id.as_str()) {
            return Err(format!("duplicate automation trigger id `{id}`"));
        }
    }
    Ok(())
}

fn validate_automation_flow(
    flow: &AutomationFlowSpec,
    step_ids: &mut BTreeSet<String>,
) -> Result<(), String> {
    if flow.steps.is_empty() {
        return Err("automation loop body must contain at least one step".into());
    }
    for step in &flow.steps {
        match step {
            AutomationStepSpec::AgentEnvNode { id, node, .. } => {
                validate_local_id("automation step id", id)?;
                if !step_ids.insert(id.clone()) {
                    return Err(format!("duplicate automation step id `{id}`"));
                }
                validate_agentenv_node_ref(node)?;
            }
            AutomationStepSpec::Loop {
                id,
                loop_spec,
                body,
                ..
            } => {
                validate_local_id("automation step id", id)?;
                if !step_ids.insert(id.clone()) {
                    return Err(format!("duplicate automation step id `{id}`"));
                }
                validate_loop_spec(loop_spec)?;
                validate_automation_flow(body, step_ids)?;
            }
        }
    }
    Ok(())
}

fn automation_flow_contains_loop(flow: &AutomationFlowSpec) -> bool {
    flow.steps
        .iter()
        .any(|step| matches!(step, AutomationStepSpec::Loop { .. }))
}

fn validate_loop_spec(loop_spec: &AutomationLoopSpec) -> Result<(), String> {
    match loop_spec {
        AutomationLoopSpec::Repeat {
            input,
            stop_when,
            max_iterations,
        } => {
            validate_loop_input(input)?;
            validate_loop_stop(stop_when)?;
            if *max_iterations == 0 {
                return Err(
                    "automation repeat loop max_iterations must be greater than zero".into(),
                );
            }
        }
        AutomationLoopSpec::ForEach {
            input,
            item_alias,
            max_iterations,
        } => {
            validate_loop_input(input)?;
            validate_local_id("automation foreach item_alias", item_alias)?;
            if matches!(max_iterations, Some(0)) {
                return Err("automation foreach max_iterations must be greater than zero".into());
            }
        }
    }
    Ok(())
}

fn validate_loop_input(input: &AutomationLoopInput) -> Result<(), String> {
    match input {
        AutomationLoopInput::Trigger | AutomationLoopInput::Static { .. } => Ok(()),
        AutomationLoopInput::StepOutput { step_id, path } => {
            validate_local_id("automation loop input step_id", step_id)?;
            if path.as_deref().is_some_and(|value| value.trim().is_empty()) {
                return Err("automation loop input path must not be empty".into());
            }
            Ok(())
        }
    }
}

fn validate_loop_stop(stop: &AutomationLoopStopSpec) -> Result<(), String> {
    match stop {
        AutomationLoopStopSpec::OutputEquals { path, .. } => {
            validate_json_path("loop stop path", path)
        }
        AutomationLoopStopSpec::OutputMatches { path, pattern } => {
            validate_json_path("loop stop path", path)?;
            regex::Regex::new(pattern)
                .map_err(|error| format!("loop stop regex invalid: {error}"))?;
            Ok(())
        }
        AutomationLoopStopSpec::AgentEnvNode { node } => validate_agentenv_node_ref(node),
    }
}

fn validate_agentenv_node_ref(node: &AgentEnvNodeRef) -> Result<(), String> {
    if node.node_type.trim().is_empty() {
        return Err("agentenv node_type must not be empty".into());
    }
    if node.node_type.trim() != node.node_type
        || node
            .node_type
            .chars()
            .any(|ch| ch.is_ascii_control() || ch.is_ascii_whitespace())
    {
        return Err("agentenv node_type must not include whitespace".into());
    }
    if node
        .name
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err("agentenv node name must not be empty".into());
    }
    Ok(())
}

fn validate_compiled_agentenv_workflow(workflow: &CompiledAgentEnvWorkflow) -> Result<(), String> {
    match &workflow.role {
        CompiledWorkflowRole::Root => {}
        CompiledWorkflowRole::LoopBody { step_id }
        | CompiledWorkflowRole::Continuation { step_id }
        | CompiledWorkflowRole::Helper { step_id } => {
            validate_local_id("compiled workflow role step_id", step_id)?;
        }
    }
    if let Some(workflow_id) = &workflow.workflow_id {
        validate_agentenv_workflow_id("compiled workflow workflow_id", workflow_id)?;
    }
    if let Some(hash) = &workflow.definition_hash {
        validate_hash_string("compiled workflow definition_hash", hash)?;
    }
    Ok(())
}

fn validate_hash_string(label: &str, hash: &str) -> Result<(), String> {
    let Some(hex) = hash.strip_prefix("sha256:") else {
        return Err(format!("{label} must start with `sha256:`"));
    };
    if hex.len() != 64 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(format!("{label} must be a SHA-256 hex digest"));
    }
    Ok(())
}

fn validate_local_id(label: &str, id: &str) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(format!(
            "{label} must contain only ASCII letters, digits, `_`, or `-`"
        ));
    }
    Ok(())
}

fn validate_json_path(label: &str, path: &str) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if path.split('.').any(|part| part.trim().is_empty()) {
        return Err(format!("{label} must not contain empty path segments"));
    }
    Ok(())
}

fn validate_slug(label: &str, slug: &str) -> Result<(), String> {
    if slug.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if !slug
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(format!("{label} must be kebab-case ASCII"));
    }
    Ok(())
}

fn validate_agentenv_workflow_id(label: &str, id: &str) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if id.trim() != id {
        return Err(format!("{label} must not include surrounding whitespace"));
    }
    if !id.chars().all(|ch| {
        ch.is_ascii()
            && !ch.is_ascii_control()
            && !ch.is_ascii_whitespace()
            && !matches!(ch, '/' | '?' | '#')
    }) {
        return Err(format!(
            "{label} must be a URL-safe AgentEnv workflow runtime id"
        ));
    }
    Ok(())
}

fn validate_filter(filter: &FilterSpec) -> Result<(), String> {
    match filter {
        FilterSpec::Tagged(TaggedFilterSpec::Regex { pattern, .. }) => {
            regex::Regex::new(pattern).map_err(|error| format!("filter regex invalid: {error}"))?;
        }
        FilterSpec::Tagged(TaggedFilterSpec::Jq { expression }) => {
            if expression.trim().is_empty() {
                return Err("jq filter expression must not be empty".into());
            }
            validate_jq_like(expression)?;
        }
        FilterSpec::Tagged(TaggedFilterSpec::Any { filters }) => {
            if filters.is_empty() {
                return Err("compound filter must contain at least one filter".into());
            }
            for filter in filters {
                validate_filter(filter)?;
            }
        }
        FilterSpec::Json(shape) => {
            if !shape.is_object() {
                return Err("json filter must be an object shape".into());
            }
        }
    }
    Ok(())
}

fn validate_jq_like(expression: &str) -> Result<(), String> {
    let expression = expression.trim();
    if let Some((path, value)) = expression.split_once("==") {
        parse_jq_path(path)?;
        parse_json_literal(value.trim())?;
        return Ok(());
    }
    if let Some((path, value)) = expression.split_once("=~") {
        parse_jq_path(path)?;
        let pattern = parse_quoted(value.trim())?;
        regex::Regex::new(&pattern).map_err(|error| format!("jq regex invalid: {error}"))?;
        return Ok(());
    }
    if let Some((path, call)) = expression.split_once("|") {
        parse_jq_path(path)?;
        let call = call.trim();
        if call == "exists" {
            return Ok(());
        }
        let Some(inner) = call
            .strip_prefix("test(")
            .and_then(|rest| rest.strip_suffix(')'))
        else {
            return Err("jq filter supports only test(\"regex\") calls".into());
        };
        let pattern = parse_quoted(inner.trim())?;
        regex::Regex::new(&pattern).map_err(|error| format!("jq regex invalid: {error}"))?;
        return Ok(());
    }
    Err("jq filter supports `.path == \"value\"`, `.path =~ \"regex\"`, or `.path | test(\"regex\")`".into())
}

fn parse_jq_path(path: &str) -> Result<String, String> {
    let path = path.trim();
    path.strip_prefix('.')
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "jq path must start with `.`".to_string())
}

fn parse_quoted(value: &str) -> Result<String, String> {
    serde_json::from_str::<String>(value)
        .map_err(|error| format!("jq literal must be a JSON string: {error}"))
}

fn parse_json_literal(value: &str) -> Result<Value, String> {
    serde_json::from_str::<Value>(value)
        .map_err(|error| format!("jq literal must be valid JSON: {error}"))
}

fn validate_contact_ids(contact_ids: &[String]) -> Result<(), String> {
    for contact_id in contact_ids {
        if normalize_contact_id(contact_id).is_none() {
            return Err(format!("contact id `{contact_id}` is invalid"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        automation_spec_hash, AutomationReviewSpec, AutomationRuntimeStatus, AutomationStatus,
        CompiledPufferBinding,
    };
    use puffer_subscriptions::{FilterSpec, TaggedFilterSpec};
    use serde_json::{json, Value};
    use std::collections::BTreeMap;

    fn agentenv_node(node_type: &str) -> AgentEnvNodeRef {
        AgentEnvNodeRef {
            node_type: node_type.into(),
            name: Some(node_type.into()),
            trusted: Some(true),
            config: BTreeMap::new(),
        }
    }

    fn automation_spec_with_triggers(triggers: Vec<AutomationTriggerSpec>) -> AutomationSpec {
        AutomationSpec {
            spec_version: AUTOMATION_SPEC_VERSION,
            name: "Reply helper".into(),
            description: None,
            source: AutomationSource::Template {
                template_id: "reply-draft".into(),
                template_version: Some("1".into()),
            },
            instructions: "Draft a reply for review.".into(),
            run_location: Default::default(),
            triggers,
            flow: AutomationFlowSpec {
                steps: vec![AutomationStepSpec::AgentEnvNode {
                    id: "draft".into(),
                    node: agentenv_node("transform_js"),
                    summary: Some("Prepare draft".into()),
                }],
            },
            review: AutomationReviewSpec::default(),
        }
    }

    fn puffer_connection_trigger() -> AutomationTriggerSpec {
        AutomationTriggerSpec::PufferConnection {
            id: "incoming".into(),
            connection_slug: "telegram-user".into(),
            connector_slug: Some("telegram-login".into()),
            filter: Some(FilterSpec::Tagged(TaggedFilterSpec::Regex {
                pattern: "help".into(),
                case_insensitive: true,
            })),
            ignore_filters: Vec::new(),
            contact_ids: Vec::new(),
            summary: Some("Incoming Telegram message".into()),
        }
    }

    #[test]
    fn automation_spec_accepts_repeat_loop_with_stop_condition() {
        let mut spec = automation_spec_with_triggers(vec![puffer_connection_trigger()]);
        spec.flow.steps = vec![AutomationStepSpec::Loop {
            id: "retry-until-done".into(),
            loop_spec: AutomationLoopSpec::Repeat {
                input: AutomationLoopInput::Trigger,
                stop_when: AutomationLoopStopSpec::OutputEquals {
                    path: "done".into(),
                    value: Value::Bool(true),
                },
                max_iterations: 3,
            },
            body: AutomationFlowSpec {
                steps: vec![AutomationStepSpec::AgentEnvNode {
                    id: "body-draft".into(),
                    node: agentenv_node("transform_js"),
                    summary: None,
                }],
            },
            summary: Some("Retry until done".into()),
        }];

        validate_automation_spec(&spec).unwrap();
    }

    #[test]
    fn automation_repeat_loop_requires_bounded_iterations() {
        let mut spec = automation_spec_with_triggers(vec![puffer_connection_trigger()]);
        spec.flow.steps = vec![AutomationStepSpec::Loop {
            id: "retry".into(),
            loop_spec: AutomationLoopSpec::Repeat {
                input: AutomationLoopInput::Trigger,
                stop_when: AutomationLoopStopSpec::OutputEquals {
                    path: "done".into(),
                    value: Value::Bool(true),
                },
                max_iterations: 0,
            },
            body: AutomationFlowSpec {
                steps: vec![AutomationStepSpec::AgentEnvNode {
                    id: "body".into(),
                    node: agentenv_node("transform_js"),
                    summary: None,
                }],
            },
            summary: None,
        }];

        let error = validate_automation_spec(&spec).unwrap_err();

        assert!(error.contains("max_iterations"));
    }

    #[test]
    fn automation_loop_mvp_rejects_agentenv_trigger() {
        let mut spec = automation_spec_with_triggers(vec![AutomationTriggerSpec::AgentEnvNode {
            id: "webhook".into(),
            node: agentenv_node("webhook"),
            summary: None,
        }]);
        spec.flow.steps = vec![AutomationStepSpec::Loop {
            id: "retry".into(),
            loop_spec: AutomationLoopSpec::Repeat {
                input: AutomationLoopInput::Trigger,
                stop_when: AutomationLoopStopSpec::OutputEquals {
                    path: "done".into(),
                    value: Value::Bool(true),
                },
                max_iterations: 2,
            },
            body: AutomationFlowSpec {
                steps: vec![AutomationStepSpec::AgentEnvNode {
                    id: "body".into(),
                    node: agentenv_node("transform_js"),
                    summary: None,
                }],
            },
            summary: None,
        }];

        let error = validate_automation_spec(&spec).unwrap_err();

        assert!(error.contains("loop automations"));
    }

    #[test]
    fn automation_spec_hash_excludes_record_runtime_metadata() {
        let spec = automation_spec_with_triggers(vec![puffer_connection_trigger()]);
        let first_hash = automation_spec_hash(&spec).unwrap();
        let mut record = AutomationRecord {
            id: "reply-helper".into(),
            status: AutomationStatus::Paused,
            revision: 1,
            spec: spec.clone(),
            runtime: AutomationRuntimeState {
                spec_hash: Some(first_hash.clone()),
                compiled_revision: Some(1),
                status: AutomationRuntimeStatus::Deployed,
                agentenv_workflows: vec![CompiledAgentEnvWorkflow {
                    role: CompiledWorkflowRole::Root,
                    workflow_id: Some("Wf_01HX.Runtime:123".into()),
                    definition_hash: Some(first_hash.clone()),
                    deployed: true,
                }],
                puffer_bindings: vec![CompiledPufferBinding {
                    trigger_id: "incoming".into(),
                    binding_slug: "reply-helper-binding".into(),
                }],
                last_error: None,
            },
            created_at_ms: 1,
            updated_at_ms: 2,
        };

        record.revision = 2;
        record.updated_at_ms = 3;
        record.runtime.status = AutomationRuntimeStatus::Stale;
        record.runtime.last_error = Some("stale".into());

        assert_eq!(automation_spec_hash(&record.spec).unwrap(), first_hash);

        record.spec.instructions = "Draft a shorter reply for review.".into();
        assert_ne!(automation_spec_hash(&record.spec).unwrap(), first_hash);
    }

    #[test]
    fn compiled_workflow_roles_validate_step_ids() {
        let spec = automation_spec_with_triggers(vec![puffer_connection_trigger()]);
        let record = AutomationRecord {
            id: "reply-helper".into(),
            status: AutomationStatus::Paused,
            revision: 1,
            spec,
            runtime: AutomationRuntimeState {
                spec_hash: Some(
                    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .into(),
                ),
                compiled_revision: Some(1),
                status: AutomationRuntimeStatus::DraftSynced,
                agentenv_workflows: vec![CompiledAgentEnvWorkflow {
                    role: CompiledWorkflowRole::LoopBody {
                        step_id: "bad step id".into(),
                    },
                    workflow_id: None,
                    definition_hash: None,
                    deployed: false,
                }],
                puffer_bindings: Vec::new(),
                last_error: None,
            },
            created_at_ms: 1,
            updated_at_ms: 1,
        };

        let error = validate_automation_record(&record).unwrap_err();

        assert!(error.contains("step_id"));
    }

    #[test]
    fn automation_record_accepts_runtime_artifacts_that_reference_spec_elements() {
        let mut spec = automation_spec_with_triggers(vec![puffer_connection_trigger()]);
        spec.flow.steps = vec![AutomationStepSpec::Loop {
            id: "retry".into(),
            loop_spec: AutomationLoopSpec::Repeat {
                input: AutomationLoopInput::Trigger,
                stop_when: AutomationLoopStopSpec::OutputEquals {
                    path: "done".into(),
                    value: Value::Bool(true),
                },
                max_iterations: 2,
            },
            body: AutomationFlowSpec {
                steps: vec![AutomationStepSpec::AgentEnvNode {
                    id: "body".into(),
                    node: agentenv_node("transform_js"),
                    summary: None,
                }],
            },
            summary: None,
        }];
        let record = AutomationRecord {
            id: "reply-helper".into(),
            status: AutomationStatus::Paused,
            revision: 1,
            spec,
            runtime: AutomationRuntimeState {
                spec_hash: None,
                compiled_revision: Some(1),
                status: AutomationRuntimeStatus::DraftSynced,
                agentenv_workflows: vec![
                    CompiledAgentEnvWorkflow {
                        role: CompiledWorkflowRole::Root,
                        workflow_id: None,
                        definition_hash: None,
                        deployed: false,
                    },
                    CompiledAgentEnvWorkflow {
                        role: CompiledWorkflowRole::LoopBody {
                            step_id: "retry".into(),
                        },
                        workflow_id: None,
                        definition_hash: None,
                        deployed: false,
                    },
                    CompiledAgentEnvWorkflow {
                        role: CompiledWorkflowRole::Helper {
                            step_id: "body".into(),
                        },
                        workflow_id: None,
                        definition_hash: None,
                        deployed: false,
                    },
                ],
                puffer_bindings: vec![CompiledPufferBinding {
                    trigger_id: "incoming".into(),
                    binding_slug: "reply-helper-binding".into(),
                }],
                last_error: None,
            },
            created_at_ms: 1,
            updated_at_ms: 1,
        };

        validate_automation_record(&record).unwrap();
    }

    #[test]
    fn compiled_loop_body_must_reference_loop_step() {
        let spec = automation_spec_with_triggers(vec![puffer_connection_trigger()]);
        let record = AutomationRecord {
            id: "reply-helper".into(),
            status: AutomationStatus::Paused,
            revision: 1,
            spec,
            runtime: AutomationRuntimeState {
                spec_hash: None,
                compiled_revision: Some(1),
                status: AutomationRuntimeStatus::DraftSynced,
                agentenv_workflows: vec![CompiledAgentEnvWorkflow {
                    role: CompiledWorkflowRole::LoopBody {
                        step_id: "draft".into(),
                    },
                    workflow_id: None,
                    definition_hash: None,
                    deployed: false,
                }],
                puffer_bindings: Vec::new(),
                last_error: None,
            },
            created_at_ms: 1,
            updated_at_ms: 1,
        };

        let error = validate_automation_record(&record).unwrap_err();

        assert!(error.contains("automation loop step"));
    }

    #[test]
    fn compiled_step_roles_must_reference_existing_steps() {
        let spec = automation_spec_with_triggers(vec![puffer_connection_trigger()]);
        let record = AutomationRecord {
            id: "reply-helper".into(),
            status: AutomationStatus::Paused,
            revision: 1,
            spec,
            runtime: AutomationRuntimeState {
                spec_hash: None,
                compiled_revision: Some(1),
                status: AutomationRuntimeStatus::DraftSynced,
                agentenv_workflows: vec![CompiledAgentEnvWorkflow {
                    role: CompiledWorkflowRole::Continuation {
                        step_id: "missing".into(),
                    },
                    workflow_id: None,
                    definition_hash: None,
                    deployed: false,
                }],
                puffer_bindings: Vec::new(),
                last_error: None,
            },
            created_at_ms: 1,
            updated_at_ms: 1,
        };

        let error = validate_automation_record(&record).unwrap_err();

        assert!(error.contains("automation step"));
    }

    #[test]
    fn compiled_puffer_bindings_must_reference_puffer_connection_triggers() {
        let spec = automation_spec_with_triggers(vec![AutomationTriggerSpec::Manual {
            id: "manual".into(),
            summary: None,
        }]);
        let record = AutomationRecord {
            id: "reply-helper".into(),
            status: AutomationStatus::Paused,
            revision: 1,
            spec,
            runtime: AutomationRuntimeState {
                spec_hash: None,
                compiled_revision: Some(1),
                status: AutomationRuntimeStatus::DraftSynced,
                agentenv_workflows: Vec::new(),
                puffer_bindings: vec![CompiledPufferBinding {
                    trigger_id: "manual".into(),
                    binding_slug: "manual-binding".into(),
                }],
                last_error: None,
            },
            created_at_ms: 1,
            updated_at_ms: 1,
        };

        let error = validate_automation_record(&record).unwrap_err();

        assert!(error.contains("puffer_connection trigger"));
    }

    #[test]
    fn puffer_connection_trigger_reuses_subscription_filter_validation() {
        let mut spec = automation_spec_with_triggers(vec![puffer_connection_trigger()]);
        let AutomationTriggerSpec::PufferConnection { filter, .. } = &mut spec.triggers[0] else {
            panic!("expected puffer connection trigger");
        };
        *filter = Some(FilterSpec::Tagged(TaggedFilterSpec::Jq {
            expression: ".message | test(\"help\")".into(),
        }));

        validate_automation_spec(&spec).unwrap();
    }

    #[test]
    fn puffer_connection_trigger_rejects_invalid_json_filter_shape() {
        let mut spec = automation_spec_with_triggers(vec![puffer_connection_trigger()]);
        let AutomationTriggerSpec::PufferConnection { filter, .. } = &mut spec.triggers[0] else {
            panic!("expected puffer connection trigger");
        };
        *filter = Some(FilterSpec::Json(json!(["not", "an", "object"])));

        let error = validate_automation_spec(&spec).unwrap_err();

        assert!(error.contains("json filter"));
    }
}
