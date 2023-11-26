use std::{error::Error, panic, path::Path, str::FromStr, thread, time::Duration};

use pinenut_log::{
    encrypt::gen_echd_key_pair, extract, parse, Config, DateTime, Domain, MetaBuilder, Record,
    RecordBuilder, SecretKey,
};
use tempfile::tempdir;

#[test]
fn test_mmap_buffer_writeback() -> Result<(), Box<dyn Error>> {
    let dir = tempdir()?.path().join("test");
    let domain = Domain::new("test".to_string(), dir.to_path_buf());
    let (secret_key, public_key) = gen_echd_key_pair();

    fn record(datetime: DateTime) -> Record<'static> {
        let meta = MetaBuilder::new().datetime(datetime).build();
        RecordBuilder::new().meta(meta).content("test log").build()
    }
    let records = [
        record(DateTime::from_str("2013-11-18 13:36:57Z")?),
        record(DateTime::from_str("2013-11-18 14:00:00Z")?),
        record(DateTime::from_str("2013-11-18 14:00:12Z")?),
        record(DateTime::from_str("2013-11-18 14:00:34Z")?),
    ];

    _ = panic::catch_unwind(|| {
        let config = Config::new().key(Some(public_key.clone()));
        let logger = domain.clone().logger(config);
        for record in &records {
            logger.log(record);
            thread::sleep(Duration::from_micros(100));
        }
        // Yes, just let it panic.
        panic!();
    });

    fn parse_records(
        domain: Domain,
        dir: &Path,
        secret_key: SecretKey,
        mut callback: impl FnMut(&Record),
    ) -> Result<(), Box<dyn Error>> {
        // Extracts all records.
        let datetime_range = DateTime::from_str("2013-11-18 13:36:00Z")?
            ..=DateTime::from_str("2013-11-18 14:01:00Z")?;
        let extracted_path = dir.join("result.pine");
        extract(domain, datetime_range, &extracted_path)?;

        parse(&extracted_path, Some(secret_key), |record| {
            callback(record);
            Ok(())
        })?;

        Ok(())
    }

    let mut len = 0;
    parse_records(domain.clone(), &dir, secret_key.clone(), |_| {
        len += 1;
    })?;
    assert!(len < records.len());

    // Write back
    let config = Config::new().key(Some(public_key.clone()));
    let logger = domain.clone().logger(config);
    thread::sleep(Duration::from_micros(100));
    logger.shutdown();

    // The data of the chunk written back is incomplete (the last encrypted block is
    // lost), so the last record is corrupted and will not be called back.
    let mut index = 0;
    _ = parse_records(domain.clone(), &dir, secret_key.clone(), |record| {
        assert_eq!(record, &records[index]);
        index += 1;
    });
    assert_eq!(index, records.len() - 1);

    Ok(())
}

#[test]
fn test_mmap_buffer_no_need_to_writeback() -> Result<(), Box<dyn Error>> {
    let dir = tempdir()?.path().join("test");
    let domain = Domain::new("test".to_string(), dir.to_path_buf());
    let (secret_key, public_key) = gen_echd_key_pair();

    fn record(datetime: DateTime) -> Record<'static> {
        let meta = MetaBuilder::new().datetime(datetime).build();
        RecordBuilder::new().meta(meta).content("test log").build()
    }
    let records = [
        record(DateTime::from_str("2013-11-18 13:36:57Z")?),
        record(DateTime::from_str("2013-11-18 14:00:00Z")?),
        record(DateTime::from_str("2013-11-18 14:00:12Z")?),
        record(DateTime::from_str("2013-11-18 14:00:34Z")?),
    ];

    _ = panic::catch_unwind(|| {
        let config = Config::new().key(Some(public_key.clone()));
        let logger = domain.clone().logger(config);
        for record in &records {
            logger.log(record);
            thread::sleep(Duration::from_micros(100));
        }
        logger.shutdown();
        // Yes, just let it panic.
        panic!();
    });

    fn parse_records(
        domain: Domain,
        dir: &Path,
        secret_key: SecretKey,
        mut callback: impl FnMut(&Record),
    ) -> Result<(), Box<dyn Error>> {
        // Extracts all records.
        let datetime_range = DateTime::from_str("2013-11-18 13:36:00Z")?
            ..=DateTime::from_str("2013-11-18 14:01:00Z")?;
        let extracted_path = dir.join("result.pine");
        extract(domain, datetime_range, &extracted_path)?;

        parse(&extracted_path, Some(secret_key), |record| {
            callback(record);
            Ok(())
        })?;

        Ok(())
    }

    let mut index = 0;
    _ = parse_records(domain.clone(), &dir, secret_key.clone(), |record| {
        assert_eq!(record, &records[index]);
        index += 1;
    });
    assert_eq!(index, records.len());

    // New a `Logger`.
    let config = Config::new().key(Some(public_key.clone()));
    let logger = domain.clone().logger(config);
    thread::sleep(Duration::from_micros(100));
    logger.shutdown();

    // No records were written back.
    let mut index = 0;
    _ = parse_records(domain.clone(), &dir, secret_key.clone(), |record| {
        assert_eq!(record, &records[index]);
        index += 1;
    });
    assert_eq!(index, records.len());

    Ok(())
}
