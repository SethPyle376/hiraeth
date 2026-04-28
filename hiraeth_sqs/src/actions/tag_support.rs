use std::collections::HashMap;

use crate::error::SqsError;

const MAX_TAGS_PER_QUEUE: usize = 50;
const MAX_TAG_KEY_LENGTH: usize = 128;
const MAX_TAG_VALUE_LENGTH: usize = 256;

pub(crate) fn validate_tags(
    tags: &HashMap<String, String>,
    allow_empty: bool,
) -> Result<(), SqsError> {
    if !allow_empty && tags.is_empty() {
        return Err(SqsError::BadRequest(
            "Tags must contain at least one entry".to_string(),
        ));
    }

    if tags.len() > MAX_TAGS_PER_QUEUE {
        return Err(SqsError::BadRequest(format!(
            "A queue can have at most {MAX_TAGS_PER_QUEUE} tags"
        )));
    }

    for (key, value) in tags {
        validate_tag_key(key)?;
        if value.chars().count() > MAX_TAG_VALUE_LENGTH {
            return Err(SqsError::BadRequest(format!(
                "Tag value for '{}' must be at most {} characters",
                key, MAX_TAG_VALUE_LENGTH
            )));
        }
    }

    Ok(())
}

pub(crate) fn validate_tag_keys(tag_keys: &[String], allow_empty: bool) -> Result<(), SqsError> {
    if !allow_empty && tag_keys.is_empty() {
        return Err(SqsError::BadRequest(
            "TagKeys must contain at least one entry".to_string(),
        ));
    }

    if tag_keys.len() > MAX_TAGS_PER_QUEUE {
        return Err(SqsError::BadRequest(format!(
            "TagKeys can contain at most {MAX_TAGS_PER_QUEUE} entries"
        )));
    }

    for key in tag_keys {
        validate_tag_key(key)?;
    }

    Ok(())
}

fn validate_tag_key(key: &str) -> Result<(), SqsError> {
    let key_length = key.chars().count();

    if key_length == 0 || key_length > MAX_TAG_KEY_LENGTH {
        return Err(SqsError::BadRequest(format!(
            "Tag keys must be between 1 and {} characters",
            MAX_TAG_KEY_LENGTH
        )));
    }

    if key.starts_with("aws:") {
        return Err(SqsError::BadRequest(
            "Tag keys cannot start with the reserved aws: prefix".to_string(),
        ));
    }

    Ok(())
}
