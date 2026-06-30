use anyhow::{bail, Result};
use puffer_provider_registry::{MediaBatchDescriptor, MediaBatchMode};

/// Describes a complete image generation execution plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageGenerationPlan {
    pub(crate) calls: Vec<ImageCallPlan>,
}

/// Describes one provider request within an image generation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageCallPlan {
    pub(crate) requested_count: u8,
}

/// Maximum image count puffer's generation pipeline accepts for one logical
/// request. Per-image mode fans this out to N parallel calls; grouped mode
/// issues a single call of this size. Resolution (`resolved_count`) enforces
/// the same ceiling, so a grouped request cannot exceed it even though the
/// upstream model (e.g. Seedream) may natively support more per call.
const MAX_IMAGE_GENERATION_COUNT: u8 = 9;

/// Validates the supported workflow image generation count range.
pub fn validate_image_generation_count(requested_count: u8) -> Result<()> {
    if requested_count == 0 || requested_count > MAX_IMAGE_GENERATION_COUNT {
        bail!("image generation count must be between 1 and {MAX_IMAGE_GENERATION_COUNT}");
    }
    Ok(())
}

/// Plans image generation provider calls for a requested output count.
pub(crate) fn plan_image_generation(
    requested_count: u8,
    batch: &MediaBatchDescriptor,
) -> Result<ImageGenerationPlan> {
    let call_counts = match batch.mode {
        MediaBatchMode::PerImage => {
            validate_image_generation_count(requested_count)?;
            vec![1; requested_count as usize]
        }
        MediaBatchMode::Exact => {
            validate_image_generation_count(requested_count)?;
            let limit = batch.max_images_per_call.unwrap_or(0);
            if limit < 2 {
                bail!("exact image batch mode requires max_images_per_call of at least 2");
            }
            split_exact_batches(requested_count, limit)
        }
        MediaBatchMode::Grouped => {
            // One call of size N — bounded by the model's declared per-call cap
            // and the pipeline ceiling, whichever is smaller.
            let cap = batch
                .max_images_per_call
                .unwrap_or(MAX_IMAGE_GENERATION_COUNT)
                .min(MAX_IMAGE_GENERATION_COUNT);
            if requested_count == 0 || requested_count > cap {
                bail!("grouped image generation count must be between 1 and {cap}");
            }
            vec![requested_count]
        }
    };

    Ok(ImageGenerationPlan {
        calls: call_counts
            .into_iter()
            .map(|requested_count| ImageCallPlan { requested_count })
            .collect(),
    })
}

fn split_exact_batches(total: u8, limit: u8) -> Vec<u8> {
    let mut remaining = total;
    let mut counts = Vec::new();
    while remaining > 0 {
        let count = remaining.min(limit);
        counts.push(count);
        remaining -= count;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use puffer_provider_registry::MediaBatchMode;

    fn per_image_batch() -> MediaBatchDescriptor {
        MediaBatchDescriptor {
            mode: MediaBatchMode::PerImage,
            max_images_per_call: None,
            count_field: None,
            count_field_parent: None,
        }
    }

    fn exact_batch(limit: u8) -> MediaBatchDescriptor {
        MediaBatchDescriptor {
            mode: MediaBatchMode::Exact,
            max_images_per_call: Some(limit),
            count_field: None,
            count_field_parent: None,
        }
    }

    fn grouped_batch(limit: u8) -> MediaBatchDescriptor {
        MediaBatchDescriptor {
            mode: MediaBatchMode::Grouped,
            max_images_per_call: Some(limit),
            count_field: Some("max_images".to_string()),
            count_field_parent: Some("sequential_image_generation_options".to_string()),
        }
    }

    fn counts(plan: &ImageGenerationPlan) -> Vec<u8> {
        plan.calls.iter().map(|call| call.requested_count).collect()
    }

    #[test]
    fn per_image_plan_splits_every_image_into_its_own_call() {
        let plan = plan_image_generation(4, &per_image_batch()).expect("plan");

        assert_eq!(counts(&plan), vec![1, 1, 1, 1]);
    }

    #[test]
    fn exact_plan_splits_by_declared_limit() {
        let plan = plan_image_generation(4, &exact_batch(2)).expect("plan");

        assert_eq!(counts(&plan), vec![2, 2]);
    }

    #[test]
    fn exact_plan_uses_remainder_call() {
        let plan = plan_image_generation(4, &exact_batch(3)).expect("plan");

        assert_eq!(counts(&plan), vec![3, 1]);
    }

    #[test]
    fn missing_batch_descriptor_defaults_to_per_image() {
        let plan =
            plan_image_generation(2, &MediaBatchDescriptor::default()).expect("default plan");

        assert_eq!(counts(&plan), vec![1, 1]);
    }

    #[test]
    fn grouped_plan_is_a_single_call_of_size_n() {
        let plan = plan_image_generation(6, &grouped_batch(15)).expect("plan");

        assert_eq!(counts(&plan), vec![6]);
    }

    #[test]
    fn grouped_allows_count_up_to_the_pipeline_cap_as_one_call() {
        let plan = plan_image_generation(9, &grouped_batch(15)).expect("plan");

        assert_eq!(counts(&plan), vec![9]);
    }

    #[test]
    fn grouped_rejects_count_over_pipeline_cap() {
        let error = plan_image_generation(10, &grouped_batch(15)).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("grouped image generation count"),
            "{error}"
        );
    }

    #[test]
    fn grouped_respects_a_smaller_declared_model_cap() {
        let error = plan_image_generation(6, &grouped_batch(5)).unwrap_err();

        assert!(error.to_string().contains("between 1 and 5"), "{error}");
    }

    #[test]
    fn rejects_requested_count_outside_supported_range() {
        let zero = plan_image_generation(0, &per_image_batch()).unwrap_err();
        let too_many = plan_image_generation(10, &per_image_batch()).unwrap_err();

        assert_eq!(
            zero.to_string(),
            "image generation count must be between 1 and 9"
        );
        assert_eq!(
            too_many.to_string(),
            "image generation count must be between 1 and 9"
        );
    }
}
