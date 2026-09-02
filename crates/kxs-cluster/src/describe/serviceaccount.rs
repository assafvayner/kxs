use super::header::write_header;
use super::util::write_list;
use super::writer::Writer;
use super::ServiceAccountSecretLookup;
use k8s_openapi::api::core::v1::ServiceAccount;

const SERVICE_ACCOUNT_NAME_ANNOTATION: &str = "kubernetes.io/service-account.name";
const SERVICE_ACCOUNT_UID_ANNOTATION: &str = "kubernetes.io/service-account.uid";

pub fn write(
    w: &mut Writer,
    service_account: &ServiceAccount,
    lookup: Option<&ServiceAccountSecretLookup>,
) {
    write_header(w, &service_account.metadata, true);

    let image_pull_secrets: Vec<String> = service_account
        .image_pull_secrets
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|reference| reference.name.as_str())
        .map(|name| referenced_name(name, lookup))
        .collect();
    write_list(w, 0, "Image pull secrets", &image_pull_secrets);

    let mountable_secrets: Vec<String> = service_account
        .secrets
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|reference| reference.name.as_deref())
        .map(|name| referenced_name(name, lookup))
        .collect();
    write_list(w, 0, "Mountable secrets", &mountable_secrets);

    write_list(w, 0, "Tokens", &token_names(service_account, lookup));
}

fn referenced_name(name: &str, lookup: Option<&ServiceAccountSecretLookup>) -> String {
    if lookup.is_some_and(|lookup| !lookup.existing_names.contains(name)) {
        format!("{name} (not found)")
    } else {
        name.to_string()
    }
}

fn token_names(
    service_account: &ServiceAccount,
    lookup: Option<&ServiceAccountSecretLookup>,
) -> Vec<String> {
    let (Some(name), Some(uid), Some(lookup)) = (
        service_account.metadata.name.as_deref(),
        service_account.metadata.uid.as_deref(),
        lookup,
    ) else {
        return Vec::new();
    };

    lookup
        .token_metadata
        .iter()
        .filter(|metadata| {
            let annotations = metadata.annotations.as_ref();
            annotations
                .and_then(|annotations| annotations.get(SERVICE_ACCOUNT_NAME_ANNOTATION))
                .map(String::as_str)
                == Some(name)
                && annotations
                    .and_then(|annotations| annotations.get(SERVICE_ACCOUNT_UID_ANNOTATION))
                    .map(String::as_str)
                    == Some(uid)
        })
        .filter_map(|metadata| metadata.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{LocalObjectReference, ObjectReference};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::{BTreeMap, BTreeSet};

    fn service_account() -> ServiceAccount {
        ServiceAccount {
            metadata: ObjectMeta {
                name: Some("builder".into()),
                namespace: Some("default".into()),
                uid: Some("sa-uid".into()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn token(name: Option<&str>, service_account: Option<&str>, uid: Option<&str>) -> ObjectMeta {
        let mut annotations = BTreeMap::new();
        if let Some(service_account) = service_account {
            annotations.insert(
                SERVICE_ACCOUNT_NAME_ANNOTATION.to_string(),
                service_account.to_string(),
            );
        }
        if let Some(uid) = uid {
            annotations.insert(SERVICE_ACCOUNT_UID_ANNOTATION.to_string(), uid.to_string());
        }
        ObjectMeta {
            name: name.map(str::to_string),
            annotations: (!annotations.is_empty()).then_some(annotations),
            ..Default::default()
        }
    }

    #[test]
    fn tokens_require_name_and_uid_and_preserve_lookup_order() {
        let service_account = service_account();
        let lookup = ServiceAccountSecretLookup {
            token_metadata: vec![
                token(Some("token-z"), Some("builder"), Some("sa-uid")),
                token(Some("stale-token"), Some("builder"), Some("old-uid")),
                token(Some("wrong-account"), Some("viewer"), Some("sa-uid")),
                token(Some("missing-uid"), Some("builder"), None),
                token(Some("token-a"), Some("builder"), Some("sa-uid")),
                token(None, Some("builder"), Some("sa-uid")),
            ],
            ..Default::default()
        };

        assert_eq!(
            token_names(&service_account, Some(&lookup)),
            ["token-z", "token-a"]
        );
    }

    #[test]
    fn referenced_secrets_are_marked_missing_only_after_a_successful_lookup() {
        let mut service_account = service_account();
        service_account.image_pull_secrets = Some(vec![
            LocalObjectReference {
                name: "registry".into(),
            },
            LocalObjectReference {
                name: "missing-pull".into(),
            },
        ]);
        service_account.secrets = Some(vec![
            ObjectReference {
                name: Some("missing-mount".into()),
                ..Default::default()
            },
            ObjectReference {
                name: Some("mounted".into()),
                ..Default::default()
            },
        ]);
        let lookup = ServiceAccountSecretLookup {
            existing_names: BTreeSet::from(["registry".into(), "mounted".into()]),
            ..Default::default()
        };

        let mut writer = Writer::new();
        write(&mut writer, &service_account, Some(&lookup));
        let output = writer.finish();
        assert!(output.contains(
            "Image pull secrets:  registry\n                     missing-pull (not found)\n"
        ));
        assert!(output.contains(
            "Mountable secrets:   missing-mount (not found)\n                     mounted\n"
        ));

        let mut writer = Writer::new();
        write(&mut writer, &service_account, None);
        assert!(!writer.finish().contains("(not found)"));
    }

    #[test]
    fn projected_tokens_do_not_appear_as_secret_tokens() {
        let mut service_account = service_account();
        service_account.automount_service_account_token = Some(true);
        let lookup = ServiceAccountSecretLookup::default();
        let mut writer = Writer::new();
        write(&mut writer, &service_account, Some(&lookup));

        let output = writer.finish();
        assert!(output.contains("Mountable secrets:   <none>\n"));
        assert!(output.contains("Tokens:              <none>\n"));
    }
}
