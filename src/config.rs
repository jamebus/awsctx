use crate::ctx;

use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::fmt;
use std::fmt::Debug;
use std::fs;
use std::fs::File;
use std::io::{BufReader, Write};
use std::io::{BufWriter, Read};
use std::path::Path;
use std::rc::Rc;

use anyhow::{anyhow, Context, Result};
use config;
use ini::Ini;

const DEFAULT_PROFILE_NAME: &str = "default";
const PROFILE_PREFIX: &str = "profile ";

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub default: bool,
    #[allow(dead_code)]
    items: Rc<HashMap<String, String>>,
}

type ConfigData = HashMap<String, Rc<HashMap<String, String>>>;

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Config {
    data: ConfigData,
    default_profile_name: Option<String>,
}

impl fmt::Display for Config {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let mut conf = Ini::new();
        let mut profile_names = Vec::from_iter(self.data.keys());

        // sort profile names by reverse order to write ascending order
        profile_names.sort();
        for profile_name in profile_names {
            let mut sec =
                conf.with_section(Some(&format!("profile {}", profile_name)));
            // NOTE: to use method chain of `&mut SectionSetter`, declare `s` before
            let mut s = sec.borrow_mut();
            let data = self.data.get(profile_name).unwrap();
            let mut data_keys = Vec::from_iter(data.keys());
            data_keys.sort();
            for data_key in data_keys {
                s = s.set(data_key, data.get(data_key).unwrap());
            }
        }

        // write default profile to section first to write last
        if let Some(default_profile_name) = &self.default_profile_name {
            let mut sec = conf.with_section(Some(DEFAULT_PROFILE_NAME));
            // NOTE: to use method chain of `&mut SectionSetter`, declare `s` before
            let mut s = sec.borrow_mut();
            let data = self.data.get(default_profile_name).unwrap();
            let mut data_keys = Vec::from_iter(data.keys());
            data_keys.sort();
            for data_key in data_keys {
                s = s.set(data_key, data.get(data_key).unwrap());
            }
        }

        let mut buf = vec![];

        {
            let mut f = BufWriter::new(&mut buf);
            conf.write_to(&mut f).unwrap();
        }
        write!(fmt, "{}", String::from_utf8(buf).unwrap())
    }
}

impl Config {
    pub fn load_config<P: AsRef<Path>>(
        config_path: P,
    ) -> Result<Self, ctx::CTXError> {
        let file = fs::File::open(config_path).map_err(|e| {
            ctx::CTXError::CannotReadConfig {
                source: Some(e.into()),
            }
        })?;

        let mut data = parse_aws_config(&file)?;
        let ck = find_default_from_parsed_aws_config(&data);
        // remove DEFAULT_KEY after retrain current key
        data.remove(DEFAULT_PROFILE_NAME);
        data.remove(&format!("{}{}", PROFILE_PREFIX, DEFAULT_PROFILE_NAME));

        let data = data
            .into_iter()
            .map(|(k, v)| {
                (k.strip_prefix(PROFILE_PREFIX).unwrap_or(&k).to_string(), v)
            })
            .collect();

        Ok(Config {
            data,
            default_profile_name: ck,
        })
    }

    fn is_default_profile(&self, name: &str) -> bool {
        self.default_profile_name
            .as_ref()
            .map(|n| n.as_str() == name)
            .unwrap_or_default()
    }

    pub fn get_profile(&self, name: &str) -> Result<Profile, ctx::CTXError> {
        let items =
            self.data.get(name).ok_or(ctx::CTXError::NoSuchProfile {
                profile: name.to_string(),
                source: Some(anyhow!(format!(
                    "unknown context name: {}",
                    name
                ))),
            })?;
        Ok(Profile {
            name: name.into(),
            items: items.clone(),
            default: self.is_default_profile(name),
        })
    }

    pub fn get_default_profile(&self) -> Result<Profile, ctx::CTXError> {
        let name = self
            .default_profile_name
            .as_ref()
            .ok_or(ctx::CTXError::NoActiveContext { source: None })?;
        self.get_profile(name)
    }

    pub fn set_default_profile(
        &mut self,
        name: &str,
    ) -> Result<Profile, ctx::CTXError> {
        let items =
            self.data.get(name).ok_or(ctx::CTXError::NoSuchProfile {
                profile: name.to_string(),
                source: Some(anyhow!(format!(
                    "unknown context name: {}",
                    name
                ))),
            })?;
        self.default_profile_name = Some(name.to_string());
        Ok(Profile {
            name: name.into(),
            items: items.clone(),
            default: true,
        })
    }

    pub fn dump_config<P: AsRef<Path>>(
        &self,
        config_path: P,
    ) -> Result<(), ctx::CTXError> {
        let mut file = fs::File::create(config_path).map_err(|e| {
            ctx::CTXError::CannotWriteConfig {
                source: Some(e.into()),
            }
        })?;
        file.write_all(self.to_string().as_bytes()).map_err(|e| {
            ctx::CTXError::CannotWriteConfig {
                source: Some(e.into()),
            }
        })?;
        file.flush().map_err(|e| ctx::CTXError::CannotWriteConfig {
            source: Some(e.into()),
        })?;
        Ok(())
    }

    pub fn list_profiles(&self) -> Vec<Profile> {
        let mut profiles = self
            .data
            .iter()
            .map(|(name, items)| Profile {
                name: name.to_string(),
                items: items.clone(),
                default: self.is_default_profile(name),
            })
            .collect::<Vec<Profile>>();
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        profiles
    }
}

fn parse_aws_config(file: &File) -> Result<ConfigData, ctx::CTXError> {
    let mut buf_reader = BufReader::new(file);
    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents).map_err(|e| {
        ctx::CTXError::CannotReadConfig {
            source: Some(e.into()),
        }
    })?;
    let c = config::Config::builder()
        .add_source(config::File::from_str(
            contents.as_str(),
            config::FileFormat::Ini,
        ))
        .build()
        .context("failed to load aws config".to_string())
        .map_err(|e| ctx::CTXError::ConfigIsBroken { source: Some(e) })?;

    c.try_deserialize::<HashMap<String, HashMap<String, String>>>()
        .context("failed to deserialize config".to_string())
        .map_or_else(
            |e| Err(ctx::CTXError::ConfigIsBroken { source: Some(e) }),
            |h| Ok(h.into_iter().map(|(k, v)| (k, Rc::new(v))).collect()),
        )
}

fn find_default_from_parsed_aws_config(data: &ConfigData) -> Option<String> {
    let default_items = data.get(DEFAULT_PROFILE_NAME)?;
    for (name, item) in data {
        if name != DEFAULT_PROFILE_NAME && item == default_items {
            if let Some(profile_name) = name.strip_prefix(PROFILE_PREFIX) {
                return Some(profile_name.into());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::io::{Seek, SeekFrom};

    use maplit::hashmap;
    use rstest::*;
    use tempfile::NamedTempFile;

    use super::*;

    #[fixture]
    pub fn aws_config_text() -> String {
        r#"[profile bar]
output=YYYYYYYYYYY
region=YYYYYYYYYYY

[profile foo]
output=XXXXXXXXXXX
region=XXXXXXXXXXX

[default]
output=XXXXXXXXXXX
region=XXXXXXXXXXX
"#
        .to_string()
    }

    #[fixture]
    pub fn aws_config_text_without_default() -> String {
        r#"[profile bar]
output=YYYYYYYYYYY
region=YYYYYYYYYYY

[profile foo]
output=XXXXXXXXXXX
region=XXXXXXXXXXX
"#
        .to_string()
    }

    #[fixture(text = aws_config_text())]
    pub fn aws_config(text: String) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", text).unwrap();
        f.flush().unwrap();
        f.seek(SeekFrom::Start(0)).unwrap();
        f
    }

    #[fixture(aws_config = aws_config(aws_config_text()))]
    pub fn parsed_aws_config(aws_config: NamedTempFile) -> ConfigData {
        parse_aws_config(aws_config.as_file()).unwrap()
    }

    #[fixture]
    pub fn foo_profile_items() -> Rc<HashMap<String, String>> {
        Rc::new(hashmap! {
            "region".to_string() => "XXXXXXXXXXX".to_string(),
            "output".to_string() => "XXXXXXXXXXX".to_string(),
        })
    }

    #[fixture]
    pub fn bar_profile_items() -> Rc<HashMap<String, String>> {
        Rc::new(hashmap! {
            "region".to_string() => "YYYYYYYYYYY".to_string(),
            "output".to_string() => "YYYYYYYYYYY".to_string(),
        })
    }

    #[fixture]
    pub fn config() -> Config {
        Config {
            data: hashmap! {
                "foo".to_string() => foo_profile_items(),
                "bar".to_string() => bar_profile_items(),
            },
            default_profile_name: Some("foo".to_string()),
        }
    }

    #[fixture]
    pub fn config_without_default() -> Config {
        Config {
            data: hashmap! {
                "foo".to_string() => foo_profile_items(),
                "bar".to_string() => bar_profile_items(),
            },
            default_profile_name: None,
        }
    }

    #[rstest]
    fn test_parse_aws_config(aws_config: NamedTempFile) {
        let expect = hashmap! {
            "profile foo".to_string() => foo_profile_items(),
            "profile bar".to_string() => bar_profile_items(),
            "default".to_string() => foo_profile_items(),
        };
        let actual = parse_aws_config(aws_config.as_file()).unwrap();
        assert_eq!(expect, actual);
    }

    #[rstest(::trace)]
    #[case(
        parsed_aws_config(aws_config(aws_config_text())),
        Some("foo".to_string())
    )]
    #[case(
        parsed_aws_config(aws_config(aws_config_text_without_default())),
        None
    )]
    fn test_find_default_from_parsed_aws_config(
        #[case] parsed_aws_config: ConfigData,
        #[case] expect: Option<String>,
    ) {
        let actual = find_default_from_parsed_aws_config(&parsed_aws_config);
        assert_eq!(expect, actual);
    }

    #[rstest(::trace)]
    #[case(aws_config(aws_config_text()), config())]
    #[case(
        aws_config(aws_config_text_without_default()),
        config_without_default()
    )]

    fn test_config_load_config(
        #[case] aws_config: NamedTempFile,
        #[case] expect: Config,
    ) {
        let actual = Config::load_config(aws_config.path()).unwrap();
        assert_eq!(expect, actual);
    }

    #[rstest(::trace)]
    #[case(
        "foo",
        Ok(Profile {
            name: "foo".to_string(),
            default: true,
            items: foo_profile_items(),
        })
    )]
    #[case(
        "bar",
        Ok(Profile {
            name: "bar".to_string(),
            default: false,
            items: bar_profile_items(),
        })
    )]
    #[case("unknown", Err(ctx::CTXError::NoSuchProfile {
            profile: name.to_string(),
            source: Some(anyhow!(format!("unknown context name: {}", name))),
        }))]
    fn test_config_get_profile(
        config: Config,
        #[case] name: &str,
        #[case] expect: Result<Profile, ctx::CTXError>,
    ) {
        let actual = config.get_profile(name);
        match (expect, actual) {
            (Ok(expect), Ok(actual)) => assert_eq!(expect, actual),
            (Err(expect), Err(actual)) => match (&expect, &actual) {
                (
                    ctx::CTXError::NoSuchProfile {
                        profile: expect_profile,
                        source: _expect_source,
                    },
                    ctx::CTXError::NoSuchProfile {
                        profile: actual_profile,
                        source: _actual_source,
                    },
                ) => {
                    assert_eq!(expect_profile, actual_profile);
                }
                _ => panic!("unexpected error: {}", actual),
            },
            _ => panic!("expect and actual are not match"),
        }
    }

    #[rstest(::trace)]
    #[case(
        config(),
        Ok(Profile {
            name: "foo".to_string(),
            default: true,
            items: foo_profile_items(),
        })
    )]
    #[case(config_without_default(), Err(ctx::CTXError::NoActiveContext { source: None }))]
    fn test_config_get_default_profile(
        #[case] config: Config,
        #[case] expect: Result<Profile, ctx::CTXError>,
    ) {
        let actual = config.get_default_profile();
        match (expect, actual) {
            (Ok(expect), Ok(actual)) => assert_eq!(expect, actual),
            (Err(expect), Err(actual)) => match (&expect, &actual) {
                (
                    ctx::CTXError::NoActiveContext {
                        source: _expect_source,
                    },
                    ctx::CTXError::NoActiveContext {
                        source: _actual_source,
                    },
                ) => (),
                _ => panic!("unexpected error: {}", actual),
            },
            _ => panic!("expect and actual are not match"),
        }
    }

    #[rstest(::trace)]
    #[case(
        "foo",
        Ok(Profile {
            name: "foo".to_string(),
            default: true,
            items: foo_profile_items(),
        })
    )]
    #[case(
        "bar",
        Ok(Profile {
            name: "bar".to_string(),
            default: true,
            items: bar_profile_items(),
        })
    )]
    #[case("unknown", Err(ctx::CTXError::NoSuchProfile {
            profile: name.to_string(),
            source: Some(anyhow!(format!("unknown context name: {}", name))),
        }))]
    fn test_config_set_default_profile(
        mut config: Config,
        #[case] name: &str,
        #[case] expect: Result<Profile, ctx::CTXError>,
    ) {
        let actual = config.set_default_profile(name);
        match (expect, actual) {
            (Ok(expect), Ok(actual)) => {
                assert_eq!(expect, actual);
                // check default profile is updated
                assert_eq!(Some(name.to_string()), config.default_profile_name);
            }
            (Err(expect), Err(actual)) => match (&expect, &actual) {
                (
                    ctx::CTXError::NoSuchProfile {
                        profile: expect_profile,
                        source: _expect_source,
                    },
                    ctx::CTXError::NoSuchProfile {
                        profile: actual_profile,
                        source: _actual_source,
                    },
                ) => {
                    assert_eq!(expect_profile, actual_profile);
                }
                _ => panic!("unexpected error: {}", actual),
            },
            _ => panic!("expect and actual are not match"),
        }
    }

    #[rstest(::trace)]
    #[case(config(), aws_config_text())]
    #[case(config_without_default(), aws_config_text_without_default())]
    fn test_config_dump_config(
        #[case] config: Config,
        #[case] aws_config_text: String,
    ) {
        let namedfile = NamedTempFile::new().unwrap();
        let expect = aws_config_text;

        config.dump_config(namedfile.path()).unwrap();
        let actual = fs::read_to_string(namedfile.path()).unwrap();
        assert_eq!(expect, actual);
    }

    #[rstest(::trace)]
    fn test_list_profiles(config: Config) {
        let expect = vec![
            Profile {
                name: "bar".to_string(),
                default: false,
                items: bar_profile_items(),
            },
            Profile {
                name: "foo".to_string(),
                default: true,
                items: foo_profile_items(),
            },
        ];

        let actual = config.list_profiles();
        assert_eq!(expect, actual);
    }
}
