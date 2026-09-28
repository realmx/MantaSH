//! Presentation helpers for transfer paths and progress; transport targets stay on TransferRecord.
use crate::model::TransferRecord;

/// Display order is source then destination for either transfer direction.
pub(super) fn endpoints(record: &TransferRecord) -> (&str, &str) {
    if record.upload {
        (&record.local, &record.remote)
    } else {
        (&record.remote, &record.local)
    }
}

/// A short filename for scanning rows; never used as a transport target.
pub(super) fn display_name(record: &TransferRecord) -> &str {
    let source = endpoints(record).0;
    source
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(source)
}

/// Fraction of transferred bytes; zero-byte files show an empty track until completion.
pub(super) fn transfer_progress_fraction(bytes: u64, total: Option<u64>) -> Option<f32> {
    total.map(|total| {
        if total == 0 {
            0.
        } else {
            (bytes as f64 / total as f64).clamp(0., 1.) as f32
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn record(local: &str, remote: &str) -> TransferRecord {
        TransferRecord {
            id: Uuid::new_v4(),
            profile: crate::model::Profile {
                id: Uuid::new_v4(),
                name: "qa".into(),
                host: "127.0.0.1".into(),
                port: 22,
                username: "qa".into(),
            },
            upload: true,
            local: local.into(),
            remote: remote.into(),
            session: Some(Uuid::new_v4()),
            attempt: Some(Uuid::new_v4()),
            state: crate::model::TransferState::Queued,
            bytes: 0,
            total: None,
            error: None,
            timestamp: 0,
        }
    }

    #[test]
    fn transfer_display_keeps_endpoint_direction_and_unicode_name() {
        let mut task = record("/Users/中文 文件.txt", "/srv/中文 文件.txt");
        assert_eq!(
            endpoints(&task),
            (task.local.as_str(), task.remote.as_str())
        );
        assert_eq!(display_name(&task), "中文 文件.txt");
        task.upload = false;
        assert_eq!(
            endpoints(&task),
            (task.remote.as_str(), task.local.as_str())
        );
        task.remote = "C:\\Files\\报告.pdf".into();
        assert_eq!(display_name(&task), "报告.pdf");
    }
    #[test]
    fn transfer_progress_fraction_handles_zero_small_and_large_files() {
        assert_eq!(transfer_progress_fraction(0, None), None);
        assert_eq!(transfer_progress_fraction(0, Some(0)), Some(0.));
        assert_eq!(transfer_progress_fraction(0, Some(100)), Some(0.));
        assert_eq!(transfer_progress_fraction(1, Some(200)), Some(0.005));
        assert_eq!(transfer_progress_fraction(25, Some(100)), Some(0.25));
        assert_eq!(transfer_progress_fraction(50, Some(100)), Some(0.5));
        assert_eq!(transfer_progress_fraction(100, Some(100)), Some(1.));
        assert_eq!(transfer_progress_fraction(110, Some(100)), Some(1.));
        assert_eq!(
            transfer_progress_fraction(u64::MAX / 2 + 1, Some(u64::MAX)),
            Some(0.5)
        );
    }
}
