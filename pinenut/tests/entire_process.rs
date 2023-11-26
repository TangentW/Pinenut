use std::{error::Error, fs, str::FromStr, thread, time::Duration};

use pinenut_log::{
    encrypt::gen_echd_key_pair, extract, parse, Config, DateTime, Domain, MetaBuilder,
    RecordBuilder,
};
use tempfile::tempdir;

/// Entrie Process: `Log` -> `Extract` -> `Parse`.
#[test]
fn test_entire_process() -> Result<(), Box<dyn Error>> {
    let dir = tempdir()?.path().join("test");
    let domain = Domain::new("test".to_string(), dir.to_path_buf());
    let (secret_key, public_key) = gen_echd_key_pair();

    let config = Config::new().key(Some(public_key));
    let logger = domain.clone().logger(config);

    let log = |datetime: DateTime| {
        let meta = MetaBuilder::new().datetime(datetime).build();
        let record = RecordBuilder::new().meta(meta).content("test log").build();
        logger.log(&record);
        thread::sleep(Duration::from_micros(100));
        record
    };

    let records = [
        log(DateTime::from_str("2013-11-18 13:35:12Z")?),
        log(DateTime::from_str("2013-11-18 13:35:23Z")?),
        log(DateTime::from_str("2013-11-18 13:36:00Z")?),
        log(DateTime::from_str("2013-11-18 13:36:57Z")?),
        log(DateTime::from_str("2013-11-18 14:00:00Z")?),
        log(DateTime::from_str("2013-11-18 14:02:23Z")?),
        log(DateTime::from_str("2013-11-18 14:12:33Z")?),
        log(DateTime::from_str("2013-11-18 15:20:12Z")?),
    ];

    logger.shutdown();

    // 1 buffer file + 2013-11-18.13 + 2013-11-18.14 + 2013-11-18.15 + current hour.
    assert_eq!(fs::read_dir(&dir)?.count(), 5);

    // Extracts records[2..6].
    let datetime_range =
        DateTime::from_str("2013-11-18 13:36:00Z")?..=DateTime::from_str("2013-11-18 14:05:23Z")?;
    let extracted_path = dir.join("result.pine");
    extract(domain, datetime_range, &extracted_path)?;

    let mut index = 2;
    parse(&extracted_path, Some(secret_key), |record| {
        assert_eq!(record, &records[index]);
        index += 1;
        Ok(())
    })?;
    assert_eq!(index, 6);

    Ok(())
}
