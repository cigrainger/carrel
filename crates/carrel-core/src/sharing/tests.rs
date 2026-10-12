use super::*;
use crate::private_library::{ArticleMetadata, BlobReference, ContentReference, ItemIdentifier};
use proptest::prelude::*;

fn credentials() -> (DeviceCert, Keypair, PublicKey) {
    let master = Keypair::from_seed(&[11; 32]);
    let device = Keypair::from_seed(&[12; 32]);
    (
        DeviceCert::new_at_micros(&master, device.public(), 1, None),
        device,
        *Keypair::from_seed(&[13; 32]).public(),
    )
}
fn article() -> SavedItem {
    SavedItem {
        id: "a".repeat(64),
        kind: "article".into(),
        title: "A careful essay".into(),
        creators: vec!["A publisher".into()],
        primary_url: Some("https://example.org/essay".into()),
        published_at_micros: Some(42),
        language: Some("en".into()),
        summary: None,
        discovered_at_micros: 0,
        article: Some(ArticleMetadata {
            feed_url: None,
            word_count: Some(500),
            estimated_read_minutes: Some(3),
            site_name: Some("Publisher".into()),
            byline: None,
        }),
        identifiers: vec![],
        contents: vec![ContentReference {
            format: "html_readable".into(),
            blob: BlobReference {
                blob_id: "b".repeat(64),
                byte_size: 123,
            },
            fetched_at_micros: 0,
            extracted_with: Some("readability".into()),
        }],
        resources: vec![],
        current_images: Some(Vec::new()),
    }
}
fn share(note: Option<String>) -> Share {
    let (cert, device, recipient) = credentials();
    Share::sign(
        "draft-1".into(),
        recipient,
        article(),
        note,
        10,
        cert,
        &device,
    )
    .unwrap()
}

#[test]
fn contact_code_binds_certificate_and_connection_information() {
    let (cert, device, _) = credentials();
    let card = ContactCard::sign(cert.clone(), "endpoint-information".into(), &device).unwrap();
    assert_eq!(
        ContactCard::from_text(&card.to_text().unwrap()).unwrap(),
        card
    );
    let mut forged = card.clone();
    forged.address = "other endpoint".into();
    assert!(forged.verify().is_err());
    let mut forged = card.clone();
    forged.certificate.master_pubkey = *Keypair::from_seed(&[44; 32]).public();
    assert!(forged.verify().is_err());
    assert!(ContactCard::sign(cert, "endpoint".into(), &Keypair::from_seed(&[44; 32])).is_err());
    assert!(ContactCard::from_text(&"x".repeat(MAX_CONTACT_BYTES + 1)).is_err());
    assert!(ContactCard::from_text("carrel-pair:{}").is_err());
    assert!(
        ContactCard::sign(card.certificate.clone(), "x".repeat(16 * 1024 + 1), &device).is_err()
    );
    let mut unsupported = card;
    unsupported.version = 2;
    assert!(unsupported.verify().is_err());
}

#[test]
fn all_share_fields_are_signed_and_canonical_wire_data_is_bounded() {
    let original = share(Some("Worth reading — 文".into()));
    let fact = ShareFact::Share(Box::new(original.clone()));
    let bytes = fact.to_bytes().unwrap();
    assert_eq!(ShareFact::from_bytes(&bytes).unwrap(), fact);
    assert_eq!(fact.recipient(), original.recipient);
    assert_eq!(fact.author(), original.certificate.master_pubkey);
    for field in 0..8 {
        let mut forged = original.clone();
        match field {
            0 => forged.recipient = *Keypair::from_seed(&[14; 32]).public(),
            1 => forged.article.title = "Different article".into(),
            2 => forged.note = Some("Different note".into()),
            3 => forged.created_at_micros = 11,
            4 => forged.request_id = "other-token".into(),
            5 => forged.id = "f".repeat(64),
            6 => forged.article.contents[0].blob.byte_size = 124,
            _ => forged.certificate.device_name = Some("changed certificate".into()),
        }
        assert!(forged.verify().is_err(), "field {field}");
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert!(ShareFact::from_bytes(&trailing).is_err());
    assert!(ShareFact::from_bytes(&vec![0; MAX_SHARE_BYTES + 1]).is_err());
    assert!(ShareFact::from_bytes(&[]).is_err());
    let mut unsupported = original;
    unsupported.version = 2;
    assert!(unsupported.verify().is_err());
}

#[test]
fn correctly_signed_private_metadata_cannot_cross_the_social_boundary() {
    let (cert, device, recipient) = credentials();
    for field in 0..7 {
        let mut snapshot = article();
        match field {
            0 => {
                snapshot.article.as_mut().unwrap().feed_url =
                    Some("https://private.example/feed".into())
            }
            1 => snapshot.discovered_at_micros = 900,
            2 => snapshot.identifiers.push(ItemIdentifier {
                scheme: "feed_guid".into(),
                value: "private identifier".into(),
                is_canonical: false,
                discovered_at_micros: 0,
            }),
            3 => snapshot.contents[0].fetched_at_micros = 900,
            4 => snapshot.contents[0].format = "epub".into(),
            5 => snapshot.primary_url = Some("file:///private/article".into()),
            _ => {
                snapshot.contents.clear();
                snapshot.resources.push(BlobReference {
                    blob_id: "b".repeat(64),
                    byte_size: 123,
                });
            }
        }
        assert!(
            Share::sign(
                "draft".into(),
                recipient,
                snapshot,
                None,
                1,
                cert.clone(),
                &device
            )
            .is_err(),
            "private field {field}"
        );
    }
    let mut link = article();
    link.contents.clear();
    link.current_images = None;
    assert!(Share::sign("link".into(), recipient, link, None, 1, cert, &device).is_ok());
}

#[test]
fn notes_tokens_and_retractions_have_defined_boundaries() {
    let (cert, device, recipient) = credentials();
    assert!(
        Share::sign(
            "note".into(),
            recipient,
            article(),
            Some("x".repeat(MAX_SHARE_NOTE_BYTES)),
            1,
            cert.clone(),
            &device
        )
        .is_ok()
    );
    assert!(
        Share::sign(
            "note".into(),
            recipient,
            article(),
            Some("x".repeat(MAX_SHARE_NOTE_BYTES + 1)),
            1,
            cert.clone(),
            &device
        )
        .is_err()
    );
    for token in ["", "contains spaces", "bad/segment"] {
        assert!(Share::draft_id(&cert, token).is_err());
    }
    assert!(Share::draft_id(&cert, &"a".repeat(128)).is_ok());
    assert!(Share::draft_id(&cert, &"a".repeat(129)).is_err());
    let original = share(None);
    let retraction =
        ShareRetraction::sign(original.id.clone(), recipient, -999, cert.clone(), &device).unwrap();
    let fact = ShareFact::Retraction(Box::new(retraction.clone()));
    assert_eq!(
        ShareFact::from_bytes(&fact.to_bytes().unwrap()).unwrap(),
        fact
    );
    assert_eq!(fact.share_id(), original.id);
    let mut forged = retraction;
    forged.recipient = *Keypair::from_seed(&[22; 32]).public();
    assert!(forged.verify().is_err());
    assert!(ShareRetraction::sign("not-an-id".into(), recipient, 0, cert, &device).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn arbitrary_unicode_notes_round_trip_and_retries_keep_the_draft_identity(note in ".{0,500}", at in any::<i64>()) {
        let (cert,device,recipient)=credentials();
        let first=Share::sign("same-draft".into(),recipient,article(),Some(note.clone()),at,cert.clone(),&device).unwrap();
        let retry=Share::sign("same-draft".into(),recipient,article(),Some(note),at.saturating_add(1),cert,&device).unwrap();
        prop_assert_eq!(&first.id,&retry.id);
        let fact=ShareFact::Share(Box::new(first));
        prop_assert_eq!(ShareFact::from_bytes(&fact.to_bytes().unwrap()).unwrap(),fact);
    }
}
