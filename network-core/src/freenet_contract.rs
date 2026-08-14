//! Freenet contract interface implementation for contract state management.

use serde::{Deserialize, Serialize};

/// Contract interface trait for Freenet contract operations
pub trait ContractInterface {
    /// Validates contract state parameters
    fn validate_state(
        &self,
        parameters: Parameters,
        state: State,
        related: RelatedContracts,
    ) -> Result<ValidateResult, ContractError>;

    /// Updates contract state with new data
    fn update_state(
        &self,
        parameters: Parameters,
        state: State,
        data: Vec<UpdateData>,
    ) -> Result<UpdateModification, ContractError>;

    /// Summarizes contract state for efficient syncing
    fn summarize_state(
        &self,
        parameters: Parameters,
        state: State,
    ) -> Result<StateSummary, ContractError>;

    /// Gets state delta between current state and a summary
    fn get_state_delta(
        &self,
        parameters: Parameters,
        state: State,
        summary: StateSummary,
    ) -> Result<StateDelta, ContractError>;
}

/// Contract parameters wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameters {
    pub data: Vec<u8>,
}

/// Contract state wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub data: Vec<u8>,
}

/// Related contracts wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedContracts {
    pub contracts: Vec<RelatedContract>,
}

/// Related contract reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedContract {
    pub key: String,
    pub summary: Option<StateSummary>,
}

/// Validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidateResult {
    pub valid: bool,
    pub reason: Option<String>,
}

/// Update data wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateData {
    pub data: Vec<u8>,
}

/// Update modification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateModification {
    pub new_state: State,
    pub summary: Option<StateSummary>,
}

/// State summary for efficient syncing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSummary {
    pub data: Vec<u8>,
}

/// State delta for updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDelta {
    pub data: Vec<u8>,
}

/// Contract error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractError {
    pub message: String,
    pub code: Option<u32>,
}

/// Default contract implementation for social media posts
pub struct SocialPostContract;

impl ContractInterface for SocialPostContract {
    fn validate_state(
        &self,
        _parameters: Parameters,
        state: State,
        _related: RelatedContracts,
    ) -> Result<ValidateResult, ContractError> {
        // Basic validation: state should be valid JSON
        if serde_json::from_slice::<serde_json::Value>(&state.data).is_ok() {
            Ok(ValidateResult {
                valid: true,
                reason: None,
            })
        } else {
            Ok(ValidateResult {
                valid: false,
                reason: Some("Invalid JSON state".to_string()),
            })
        }
    }

    fn update_state(
        &self,
        _parameters: Parameters,
        state: State,
        data: Vec<UpdateData>,
    ) -> Result<UpdateModification, ContractError> {
        // Merge update data into existing state
        let mut current_value: serde_json::Value = serde_json::from_slice(&state.data)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

        for update in data {
            if let Ok(update_value) = serde_json::from_slice::<serde_json::Value>(&update.data) {
                if let Some(obj) = current_value.as_object_mut() {
                    if let Some(update_obj) = update_value.as_object() {
                        for (key, value) in update_obj {
                            obj.insert(key.clone(), value.clone());
                        }
                    }
                }
            }
        }

        let new_state_data = serde_json::to_vec(&current_value).map_err(|e| ContractError {
            message: format!("State serialization failed: {e}"),
            code: Some(1),
        })?;

        let summary = StateSummary {
            data: blake3::hash(&new_state_data).as_bytes().to_vec(),
        };

        Ok(UpdateModification {
            new_state: State {
                data: new_state_data,
            },
            summary: Some(summary),
        })
    }

    fn summarize_state(
        &self,
        _parameters: Parameters,
        state: State,
    ) -> Result<StateSummary, ContractError> {
        let summary = StateSummary {
            data: blake3::hash(&state.data).as_bytes().to_vec(),
        };
        Ok(summary)
    }

    fn get_state_delta(
        &self,
        _parameters: Parameters,
        state: State,
        summary: StateSummary,
    ) -> Result<StateDelta, ContractError> {
        // Simple delta: if hashes match, empty delta; otherwise full state
        let current_hash = blake3::hash(&state.data).as_bytes().to_vec();

        if current_hash == summary.data {
            Ok(StateDelta { data: vec![] })
        } else {
            Ok(StateDelta { data: state.data })
        }
    }
}

/// Contract manager for handling multiple contract types
pub struct ContractManager {
    contracts: std::collections::HashMap<String, SocialPostContract>,
}

impl ContractManager {
    pub fn new() -> Self {
        let mut manager = Self {
            contracts: std::collections::HashMap::new(),
        };

        // Register default contract types
        manager
            .contracts
            .insert("social_post".to_string(), SocialPostContract);

        manager
    }

    pub fn get_contract(&self, contract_type: &str) -> Option<&SocialPostContract> {
        self.contracts.get(contract_type)
    }
}

impl Default for ContractManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_social_post_validation() {
        let contract = SocialPostContract;
        let parameters = Parameters { data: vec![] };
        let state = State {
            data: br#"{"content":"hello","author":"test"}"#.to_vec(),
        };
        let related = RelatedContracts { contracts: vec![] };

        let result = contract.validate_state(parameters, state, related).unwrap();
        assert!(result.valid);
    }

    #[test]
    fn test_social_post_update() {
        let contract = SocialPostContract;
        let parameters = Parameters { data: vec![] };
        let state = State {
            data: br#"{"content":"hello"}"#.to_vec(),
        };
        let update_data = vec![UpdateData {
            data: br#"{"author":"test"}"#.to_vec(),
        }];

        let modification = contract
            .update_state(parameters, state, update_data)
            .unwrap();
        let updated_value: serde_json::Value =
            serde_json::from_slice(&modification.new_state.data).unwrap();

        assert_eq!(updated_value["content"], "hello");
        assert_eq!(updated_value["author"], "test");
    }

    #[test]
    fn test_state_summary() {
        let contract = SocialPostContract;
        let parameters = Parameters { data: vec![] };
        let state = State {
            data: br#"{"test":"data"}"#.to_vec(),
        };

        let summary = contract.summarize_state(parameters, state).unwrap();
        assert_eq!(summary.data.len(), 32); // BLAKE3 hash length
    }

    #[test]
    fn test_contract_manager() {
        let manager = ContractManager::new();

        assert!(manager.get_contract("social_post").is_some());
        assert!(manager.get_contract("unknown").is_none());
    }

    #[test]
    fn test_state_delta() {
        let contract = SocialPostContract;
        let parameters = Parameters { data: vec![] };
        let state = State {
            data: br#"{"test":"data"}"#.to_vec(),
        };
        let summary = StateSummary {
            data: blake3::hash(br#"{"test":"data"}"#).as_bytes().to_vec(),
        };

        let delta = contract
            .get_state_delta(parameters, state, summary)
            .unwrap();
        assert_eq!(delta.data.len(), 0); // Should be empty since hashes match
    }

    #[test]
    fn test_payload_serde_roundtrip() {
        let params = Parameters {
            data: vec![1, 2, 3],
        };
        let state = State { data: vec![4, 5] };
        let related = RelatedContracts {
            contracts: vec![RelatedContract {
                key: "k".to_string(),
                summary: Some(StateSummary { data: vec![9] }),
            }],
        };
        let update = UpdateData { data: vec![7] };
        let modification = UpdateModification {
            new_state: state.clone(),
            summary: Some(StateSummary { data: vec![8] }),
        };
        let delta = StateDelta { data: vec![6] };
        let result = ValidateResult {
            valid: false,
            reason: Some("nope".to_string()),
        };
        let error = ContractError {
            message: "boom".to_string(),
            code: Some(2),
        };

        let decoded: Parameters =
            serde_json::from_str(&serde_json::to_string(&params).unwrap()).unwrap();
        assert_eq!(decoded.data, vec![1, 2, 3]);
        let decoded: State = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(decoded.data, vec![4, 5]);
        let decoded: RelatedContracts =
            serde_json::from_str(&serde_json::to_string(&related).unwrap()).unwrap();
        assert_eq!(decoded.contracts[0].key, "k");
        assert_eq!(decoded.contracts[0].summary.as_ref().unwrap().data, vec![9]);
        let decoded: UpdateData =
            serde_json::from_str(&serde_json::to_string(&update).unwrap()).unwrap();
        assert_eq!(decoded.data, vec![7]);
        let decoded: UpdateModification =
            serde_json::from_str(&serde_json::to_string(&modification).unwrap()).unwrap();
        assert_eq!(decoded.new_state.data, vec![4, 5]);
        assert_eq!(decoded.summary.unwrap().data, vec![8]);
        let decoded: StateDelta =
            serde_json::from_str(&serde_json::to_string(&delta).unwrap()).unwrap();
        assert_eq!(decoded.data, vec![6]);
        let decoded: ValidateResult =
            serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
        assert!(!decoded.valid);
        assert_eq!(decoded.reason.as_deref(), Some("nope"));
        let decoded: ContractError =
            serde_json::from_str(&serde_json::to_string(&error).unwrap()).unwrap();
        assert_eq!(decoded.message, "boom");
        assert_eq!(decoded.code, Some(2));
    }

    #[test]
    fn test_validation_rejects_invalid_json() {
        let contract = SocialPostContract;
        let result = contract
            .validate_state(
                Parameters { data: vec![] },
                State {
                    data: b"not json".to_vec(),
                },
                RelatedContracts { contracts: vec![] },
            )
            .unwrap();
        assert!(!result.valid);
        assert_eq!(result.reason.as_deref(), Some("Invalid JSON state"));
    }

    #[test]
    fn test_state_delta_mismatch_returns_full_state() {
        let contract = SocialPostContract;
        let state_data = br#"{"a":1}"#.to_vec();
        let delta = contract
            .get_state_delta(
                Parameters { data: vec![] },
                State {
                    data: state_data.clone(),
                },
                StateSummary { data: vec![0; 32] },
            )
            .unwrap();
        assert_eq!(delta.data, state_data);
    }
}
