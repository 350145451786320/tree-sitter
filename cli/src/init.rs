use std::{
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
    str::{self, FromStr},
};

use anyhow::{anyhow, Context, Result};
use heck::{ToKebabCase, ToShoutySnakeCase, ToSnakeCase, ToUpperCamelCase};
use indoc::indoc;
use regex::Regex;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tree_sitter_generate::write_file;
use tree_sitter_loader::{
    Author, Bindings, Grammar, Links, Metadata, PackageJSON, PackageJSONAuthor,
    PackageJSONRepository, PathsJSON, TreeSitterJSON,
};
use url::Url;

const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");
const CLI_VERSION_PLACEHOLDER: &str = "CLI_VERSION";

const ABI_VERSION_MAX: usize = tree_sitter::LANGUAGE_VERSION;
const ABI_VERSION_MAX_PLACEHOLDER: &str = "ABI_VERSION_MAX";

const PARSER_NAME_PLACEHOLDER: &str = "PARSER_NAME";
const CAMEL_PARSER_NAME_PLACEHOLDER: &str = "CAMEL_PARSER_NAME";
const UPPER_PARSER_NAME_PLACEHOLDER: &str = "UPPER_PARSER_NAME";
const LOWER_PARSER_NAME_PLACEHOLDER: &str = "LOWER_PARSER_NAME";

const PARSER_DESCRIPTION_PLACEHOLDER: &str = "PARSER_DESCRIPTION";
const PARSER_LICENSE_PLACEHOLDER: &str = "PARSER_LICENSE";
const PARSER_URL_PLACEHOLDER: &str = "PARSER_URL";
const PARSER_URL_STRIPPED_PLACEHOLDER: &str = "PARSER_URL_STRIPPED";
const PARSER_VERSION_PLACEHOLDER: &str = "PARSER_VERSION";

const AUTHOR_NAME_PLACEHOLDER: &str = "PARSER_AUTHOR_NAME";
const AUTHOR_EMAIL_PLACEHOLDER: &str = "PARSER_AUTHOR_EMAIL";
const AUTHOR_URL_PLACEHOLDER: &str = "PARSER_AUTHOR_URL";

const AUTHOR_BLOCK_JS: &str = "\n  \"author\": {";
const AUTHOR_NAME_PLACEHOLDER_JS: &str = "\n    \"name\": \"PARSER_AUTHOR_NAME\",";
const AUTHOR_EMAIL_PLACEHOLDER_JS: &str = ",\n    \"email\": \"PARSER_AUTHOR_EMAIL\"";
const AUTHOR_URL_PLACEHOLDER_JS: &str = ",\n    \"url\": \"PARSER_AUTHOR_URL\"";

const AUTHOR_BLOCK_PY: &str = "\nauthors = [{";
const AUTHOR_NAME_PLACEHOLDER_PY: &str = "name = \"PARSER_AUTHOR_NAME\"";
const AUTHOR_EMAIL_PLACEHOLDER_PY: &str = ", email = \"PARSER_AUTHOR_EMAIL\"";

const AUTHOR_BLOCK_RS: &str = "\nauthors = [";
const AUTHOR_NAME_PLACEHOLDER_RS: &str = "PARSER_AUTHOR_NAME";
const AUTHOR_EMAIL_PLACEHOLDER_RS: &str = " PARSER_AUTHOR_EMAIL";

const AUTHOR_BLOCK_GRAMMAR: &str = "\n * @author ";
const AUTHOR_NAME_PLACEHOLDER_GRAMMAR: &str = "PARSER_AUTHOR_NAME";
const AUTHOR_EMAIL_PLACEHOLDER_GRAMMAR: &str = " PARSER_AUTHOR_EMAIL";

const GRAMMAR_JS_TEMPLATE: &str = include_str!("./templates/grammar.js");
const PACKAGE_JSON_TEMPLATE: &str = include_str!("./templates/package.json");
const GITIGNORE_TEMPLATE: &str = include_str!("./templates/gitignore");
const GITATTRIBUTES_TEMPLATE: &str = include_str!("./templates/gitattributes");
const EDITORCONFIG_TEMPLATE: &str = include_str!("./templates/.editorconfig");

const RUST_BINDING_VERSION: &str = env!("CARGO_PKG_VERSION");
const RUST_BINDING_VERSION_PLACEHOLDER: &str = "RUST_BINDING_VERSION";

const LIB_RS_TEMPLATE: &str = include_str!("./templates/lib.rs");
const BUILD_RS_TEMPLATE: &str = include_str!("./templates/build.rs");
const CARGO_TOML_TEMPLATE: &str = include_str!("./templates/_cargo.toml");

const INDEX_JS_TEMPLATE: &str = include_str!("./templates/index.js");
const INDEX_D_TS_TEMPLATE: &str = include_str!("./templates/index.d.ts");
const JS_BINDING_CC_TEMPLATE: &str = include_str!("./templates/js-binding.cc");
const BINDING_GYP_TEMPLATE: &str = include_str!("./templates/binding.gyp");
const BINDING_TEST_JS_TEMPLATE: &str = include_str!("./templates/binding_test.js");

const MAKEFILE_TEMPLATE: &str = include_str!("./templates/makefile");
const CMAKELISTS_TXT_TEMPLATE: &str = include_str!("./templates/cmakelists.txt");
const PARSER_NAME_H_TEMPLATE: &str = include_str!("./templates/PARSER_NAME.h");
const PARSER_NAME_PC_IN_TEMPLATE: &str = include_str!("./templates/PARSER_NAME.pc.in");

const GO_MOD_TEMPLATE: &str = include_str!("./templates/go.mod");
const BINDING_GO_TEMPLATE: &str = include_str!("./templates/binding.go");
const BINDING_TEST_GO_TEMPLATE: &str = include_str!("./templates/binding_test.go");

const SETUP_PY_TEMPLATE: &str = include_str!("./templates/setup.py");
const INIT_PY_TEMPLATE: &str = include_str!("./templates/__init__.py");
const INIT_PYI_TEMPLATE: &str = include_str!("./templates/__init__.pyi");
const PYPROJECT_TOML_TEMPLATE: &str = include_str!("./templates/pyproject.toml");
const PY_BINDING_C_TEMPLATE: &str = include_str!("./templates/py-binding.c");
const TEST_BINDING_PY_TEMPLATE: &str = include_str!("./templates/test_binding.py");

const PACKAGE_SWIFT_TEMPLATE: &str = include_str!("./templates/package.swift");
const TESTS_SWIFT_TEMPLATE: &str = include_str!("./templates/tests.swift");

pub fn path_in_ignore(repo_path: &Path) -> bool {
    [
        "bindings",
        "build",
        "examples",
        "node_modules",
        "queries",
        "script",
        "src",
        "target",
        "test",
        "types",
    ]
    .iter()
    .any(|dir| repo_path.ends_with(dir))
}

fn insert_after(
    map: Map<String, Value>,
    after: &str,
    key: &str,
    value: Value,
) -> Map<String, Value> {
    let mut entries = map.into_iter().collect::<Vec<_>>();
    let after_index = entries
        .iter()
        .position(|(k, _)| k == after)
        .unwrap_or(entries.len() - 1)
        + 1;
    entries.insert(after_index, (key.to_string(), value));
    entries.into_iter().collect()
}

#[derive(Serialize, Deserialize, Clone)]
pub struct JsonConfigOpts {
    pub name: String,
    pub camelcase: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<Url>,
    pub scope: String,
    pub file_types: Vec<String>,
    pub version: Version,
    pub license: String,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<Url>,
}

impl JsonConfigOpts {
    pub fn to_tree_sitter_json(self) -> TreeSitterJSON {
        TreeSitterJSON {
            grammars: vec![Grammar {
                name: self.name.clone(),
                camelcase: Some(self.camelcase),
                scope: self.scope,
                path: PathBuf::from("."),
                external_files: PathsJSON::Empty,
                file_types: None,
                highlights: PathsJSON::Empty,
                injections: PathsJSON::Empty,
                locals: PathsJSON::Empty,
                tags: PathsJSON::Empty,
                injection_regex: Some(format!("^{}$", self.name)),
                first_line_regex: None,
                content_regex: None,
            }],
            metadata: Metadata {
                version: self.version,
                license: Some(self.license),
                description: Some(self.description),
                authors: Some(vec![Author {
                    name: self.author,
                    email: self.email,
                    url: None,
                }]),
                links: Some(Links {
                    repository: self.repository.unwrap_or_else(|| {
                        Url::parse(&format!(
                            "https://github.com/tree-sitter/tree-sitter-{}",
                            self.name
                        ))
                        .expect("Failed to parse default repository URL")
                    }),
                    homepage: None,
                }),
                namespace: None,
            },
            bindings: Bindings::default(),
        }
    }
}

impl Default for JsonConfigOpts {
    fn default() -> Self {
        Self {
            name: String::new(),
            camelcase: String::new(),
            description: String::new(),
            repository: None,
            scope: String::new(),
            file_types: vec![],
            version: Version::from_str("0.1.0").unwrap(),
            license: String::new(),
            author: String::new(),
            email: None,
            url: None,
        }
    }
}

struct GenerateOpts<'a> {
    author_name: Option<&'a str>,
    author_email: Option<&'a str>,
    author_url: Option<&'a str>,
    license: Option<&'a str>,
    description: Option<&'a str>,
    repository: Option<&'a str>,
    version: &'a Version,
}

// TODO: remove in 0.25
// A return value of true means migration was successful, and false if not.
pub fn migrate_package_json(repo_path: &Path) -> Result<bool> {
    let (package_json_path, tree_sitter_json_path) = (
        repo_path.join("package.json"),
        repo_path.join("tree-sitter.json"),
    );

    let old_config = serde_json::from_reader::<_, PackageJSON>(
        File::open(&package_json_path)
            .with_context(|| format!("Failed to open package.json in {}", repo_path.display()))?,
    )?;

    if old_config.tree_sitter.is_none() {
        eprintln!("Failed to find `tree-sitter` section in package.json, unable to migrate");
        return Ok(false);
    }

    let name = old_config.name.replace("tree-sitter-", "");

    let new_config = TreeSitterJSON {
        grammars: old_config
            .tree_sitter
            .unwrap()
            .into_iter()
            .map(|l| Grammar {
                name: name.clone(),
                camelcase: Some(name.to_upper_camel_case()),
                scope: l.scope.unwrap_or_else(|| format!("source.{name}")),
                path: l.path,
                external_files: l.external_files,
                file_types: l.file_types,
                highlights: l.highlights,
                injections: l.injections,
                locals: l.locals,
                tags: l.tags,
                injection_regex: l.injection_regex,
                first_line_regex: l.first_line_regex,
                content_regex: l.content_regex,
            })
            .collect(),
        metadata: Metadata {
            version: old_config.version,
            license: old_config
                .license
                .map_or_else(|| Some("MIT".to_string()), Some),
            description: old_config
                .description
                .map_or_else(|| Some(format!("{name} grammar for tree-sitter")), Some),
            authors: {
                let authors = old_config
                    .author
                    .map(|a| vec![a].into_iter())
                    .unwrap_or_else(|| vec![].into_iter())
                    .chain(old_config.maintainers.unwrap_or_default())
                    .filter_map(|a| match a {
                        PackageJSONAuthor::String(s) => {
                            let mut name = s.trim().to_string();
                            if name.is_empty() {
                                return None;
                            }

                            let mut email = None;
                            let mut url = None;

                            if let Some(url_start) = name.rfind('(') {
                                if let Some(url_end) = name.rfind(')') {
                                    url = Some(name[url_start + 1..url_end].trim().to_string());
                                    name = name[..url_start].trim().to_string();
                                }
                            }

                            if let Some(email_start) = name.rfind('<') {
                                if let Some(email_end) = name.rfind('>') {
                                    email =
                                        Some(name[email_start + 1..email_end].trim().to_string());
                                    name = name[..email_start].trim().to_string();
                                }
                            }

                            Some(Author { name, email, url })
                        }
                        PackageJSONAuthor::Object { name, email, url } => {
                            if name.is_empty() {
                                None
                            } else {
                                Some(Author { name, email, url })
                            }
                        }
                    })
                    .collect::<Vec<_>>();
                if authors.is_empty() {
                    None
                } else {
                    Some(authors)
                }
            },
            links: Some(Links {
                repository: old_config
                    .repository
                    .map(|r| match r {
                        PackageJSONRepository::String(s) => {
                            if let Some(stripped) = s.strip_prefix("github:") {
                                Url::parse(&format!("https://github.com/{stripped}"))
                            } else if Regex::new(r"^[\w.-]+/[\w.-]+$").unwrap().is_match(&s) {
                                Url::parse(&format!("https://github.com/{s}"))
                            } else if let Some(stripped) = s.strip_prefix("gitlab:") {
                                Url::parse(&format!("https://gitlab.com/{stripped}"))
                            } else if let Some(stripped) = s.strip_prefix("bitbucket:") {
                                Url::parse(&format!("https://bitbucket.org/{stripped}"))
                            } else {
                                Url::parse(&s)
                            }
                        }
                        PackageJSONRepository::Object { url, .. } => Url::parse(&url),
                    })
                    .transpose()?
                    .unwrap_or_else(|| {
                        Url::parse(&format!(
                            "https://github.com/tree-sitter/tree-sitter-{name}"
                        ))
                        .expect("Failed to parse default repository URL")
                    }),
                homepage: None,
            }),
            namespace: None,
        },
        bindings: Bindings::default(),
    };

    write_file(
        &tree_sitter_json_path,
        serde_json::to_string_pretty(&new_config)?,
    )?;

    // Remove the `tree-sitter` field in-place
    let mut package_json = serde_json::from_reader::<_, Map<String, Value>>(
        File::open(&package_json_path)
            .with_context(|| format!("Failed to open package.json in {}", repo_path.display()))?,
    )
    .unwrap();
    package_json.remove("tree-sitter");
    write_file(
        &repo_path.join("package.json"),
        serde_json::to_string_pretty(&package_json)?,
    )?;

    println!("Warning: your package.json's `tree-sitter` field has been automatically migrated to the new `tree-sitter.json` config file");
    println!(
        "For more information, visit https://tree-sitter.github.io/tree-sitter/creating-parsers"
    );

    Ok(true)
}

pub fn generate_grammar_files(
    repo_path: &Path,
    language_name: &str,
    allow_update: bool,
    opts: Option<JsonConfigOpts>,
) -> Result<()> {
    let dashed_language_name = language_name.to_kebab_case();

    // TODO: remove legacy code updates in v0.24.0

    let tree_sitter_config = missing_path_else(
        repo_path.join("tree-sitter.json"),
        true,
        |path| {
            // invariant: opts is always Some when `tree-sitter.json` doesn't exist
            let Some(opts) = opts.clone() else {
                unreachable!()
            };

            let tree_sitter_json = opts.to_tree_sitter_json();
            write_file(path, serde_json::to_string_pretty(&tree_sitter_json)?)
        },
        |path| {
            // updating the config, if needed
            if let Some(opts) = opts.clone() {
                let tree_sitter_json = opts.to_tree_sitter_json();
                write_file(path, serde_json::to_string_pretty(&tree_sitter_json)?)?;
            }
            Ok(())
        },
    )?;

    let tree_sitter_config = serde_json::from_reader::<_, TreeSitterJSON>(
        File::open(tree_sitter_config.as_path())
            .with_context(|| "Failed to open tree-sitter.json")?,
    )?;

    let authors = tree_sitter_config.metadata.authors.as_ref();

    let generate_opts = GenerateOpts {
        author_name: authors
            .map(|a| a.first().map(|a| a.name.as_str()))
            .unwrap_or_default(),
        author_email: authors
            .map(|a| a.first().and_then(|a| a.email.as_deref()))
            .unwrap_or_default(),
        author_url: authors
            .map(|a| a.first().and_then(|a| a.url.as_deref()))
            .unwrap_or_default(),
        license: tree_sitter_config.metadata.license.as_deref(),
        description: tree_sitter_config.metadata.description.as_deref(),
        repository: tree_sitter_config
            .metadata
            .links
            .as_ref()
            .map(|l| l.repository.as_str()),
        version: &tree_sitter_config.metadata.version,
    };

    // Create or update package.json
    missing_path_else(
        repo_path.join("package.json"),
        allow_update,
        |path| {
            generate_file(
                path,
                PACKAGE_JSON_TEMPLATE,
                dashed_language_name.as_str(),
                &generate_opts,
            )
        },
        |path| {
            let package_json_str =
                fs::read_to_string(path).with_context(|| "Failed to read package.json")?;
            let mut package_json = serde_json::from_str::<Map<String, Value>>(&package_json_str)
                .with_context(|| "Failed to parse package.json")?;
            let mut updated = false;

            let dependencies = package_json
                .entry("dependencies".to_string())
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .unwrap();
            if dependencies.remove("nan").is_some() {
                eprintln!("Replacing nan dependency with node-addon-api in package.json");
                dependencies.insert("node-addon-api".to_string(), "^8.0.0".into());
                updated = true;
            }
            if !dependencies.contains_key("node-gyp-build") {
                eprintln!("Adding node-gyp-build dependency to package.json");
                dependencies.insert("node-gyp-build".to_string(), "^4.8.1".into());
                updated = true;
            }

            let dev_dependencies = package_json
                .entry("devDependencies".to_string())
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .unwrap();
            if !dev_dependencies.contains_key("prebuildify") {
                eprintln!("Adding prebuildify devDependency to package.json");
                dev_dependencies.insert("prebuildify".to_string(), "^6.0.1".into());
                updated = true;
            }

            let node_test = "node --test bindings/node/*_test.js";
            let scripts = package_json
                .entry("scripts".to_string())
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .unwrap();
            if !scripts.get("test").is_some_and(|v| v == node_test) {
                eprintln!("Updating package.json scripts");
                *scripts = Map::from_iter([
                    ("install".to_string(), "node-gyp-build".into()),
                    ("prestart".to_string(), "tree-sitter build --wasm".into()),
                    ("start".to_string(), "tree-sitter playground".into()),
                    ("test".to_string(), node_test.into()),
                ]);
                updated = true;
            }

            // insert `peerDependencies` after `dependencies`
            if !package_json.contains_key("peerDependencies") {
                eprintln!("Adding peerDependencies to package.json");
                package_json = insert_after(
                    package_json,
                    "dependencies",
                    "peerDependencies",
                    json!({"tree-sitter": "^0.21.1"}),
                );

                package_json = insert_after(
                    package_json,
                    "peerDependencies",
                    "peerDependenciesMeta",
                    json!({"tree_sitter": {"optional": true}}),
                );
                updated = true;
            }

            // insert `types` right after `main`
            if !package_json.contains_key("types") {
                eprintln!("Adding types to package.json");
                package_json = insert_after(package_json, "main", "types", "bindings/node".into());
                updated = true;
            }

            // insert `files` right after `keywords`
            if !package_json.contains_key("files") {
                eprintln!("Adding files to package.json");
                package_json = insert_after(
                    package_json,
                    "keywords",
                    "files",
                    json!([
                        "grammar.js",
                        "binding.gyp",
                        "prebuilds/**",
                        "bindings/node/*",
                        "queries/*",
                        "src/**",
                        "*.wasm"
                    ]),
                );
                updated = true;
            }

            if updated {
                let mut package_json_str = serde_json::to_string_pretty(&package_json)?;
                package_json_str.push('\n');
                write_file(path, package_json_str)?;
            }

            Ok(())
        },
    )?;

    // Do not create a grammar.js file in a repo with multiple language configs
    if !tree_sitter_config.has_multiple_language_configs() {
        missing_path(repo_path.join("grammar.js"), |path| {
            generate_file(path, GRAMMAR_JS_TEMPLATE, language_name, &generate_opts)
        })?;
    }

    // Write .gitignore file
    missing_path(repo_path.join(".gitignore"), |path| {
        generate_file(path, GITIGNORE_TEMPLATE, language_name, &generate_opts)
    })?;

    // Write .gitattributes file
    missing_path(repo_path.join(".gitattributes"), |path| {
        generate_file(path, GITATTRIBUTES_TEMPLATE, language_name, &generate_opts)
    })?;

    // Write .editorconfig file
    missing_path(repo_path.join(".editorconfig"), |path| {
        generate_file(path, EDITORCONFIG_TEMPLATE, language_name, &generate_opts)
    })?;

    let bindings_dir = repo_path.join("bindings");

    // Generate Rust bindings
    if tree_sitter_config.bindings.rust {
        missing_path(bindings_dir.join("rust"), create_dir)?.apply(|path| {
            missing_path_else(
                path.join("lib.rs"),
                allow_update,
                |path| generate_file(path, LIB_RS_TEMPLATE, language_name, &generate_opts),
                |path| {
                    let lib_rs =
                        fs::read_to_string(path).with_context(|| "Failed to read lib.rs")?;
                    if !lib_rs.contains("tree_sitter_language") {
                        generate_file(path, LIB_RS_TEMPLATE, language_name, &generate_opts)?;
                        eprintln!("Updated lib.rs with `tree_sitter_language` dependency");
                    }
                    Ok(())
                },
            )?;

            missing_path_else(
                path.join("build.rs"),
                allow_update,
                |path| generate_file(path, BUILD_RS_TEMPLATE, language_name, &generate_opts),
                |path| {
                    let build_rs =
                        fs::read_to_string(path).with_context(|| "Failed to read build.rs")?;
                    if !build_rs.contains("-utf-8") {
                        let index = build_rs
                            .find("    let parser_path = src_dir.join(\"parser.c\")")
                            .ok_or_else(|| anyhow!(indoc!{
                                "Failed to auto-update build.rs with the `/utf-8` flag for windows.
                                To fix this, remove `bindings/rust/build.rs` and re-run `tree-sitter generate`"}))?;

                        let build_rs = format!(
                            "{}{}{}\n{}",
                            &build_rs[..index],
                            "    #[cfg(target_env = \"msvc\")]\n",
                            "    c_config.flag(\"-utf-8\");\n",
                            &build_rs[index..]
                        );

                        write_file(path, build_rs)?;
                        eprintln!("Updated build.rs with the /utf-8 flag for Windows compilation");
                    }
                    Ok(())
                },
            )?;

            missing_path_else(
                repo_path.join("Cargo.toml"),
                allow_update,
                |path| generate_file(path, CARGO_TOML_TEMPLATE, dashed_language_name.as_str(), &generate_opts),
                |path| {
                    let cargo_toml =
                        fs::read_to_string(path).with_context(|| "Failed to read Cargo.toml")?;
                    if !cargo_toml.contains("tree-sitter-language") {
                        let start_index = cargo_toml
                            .find("tree-sitter = \"")
                            .ok_or_else(|| anyhow!("Failed to find the `tree-sitter` dependency in Cargo.toml"))?;

                        let version_start_index = start_index + "tree-sitter = \"".len();
                        let version_end_index = cargo_toml[version_start_index..]
                            .find('\"')
                            .map(|i| i + version_start_index)
                            .ok_or_else(|| anyhow!("Failed to find the end of the `tree-sitter` version in Cargo.toml"))?;

                        let cargo_toml = format!(
                            "{}{}{}\n{}\n{}",
                            &cargo_toml[..start_index],
                            "tree-sitter-language = \"0.1.0\"",
                            &cargo_toml[version_end_index + 1..],
                            "[dev-dependencies]",
                            "tree-sitter = \"0.23\"",
                        );

                        write_file(path, cargo_toml)?;
                        eprintln!("Updated Cargo.toml with the `tree-sitter-language` dependency");
                    }
                    Ok(())
                },
            )?;

            Ok(())
        })?;
    }

    // Generate Node bindings
    if tree_sitter_config.bindings.node {
        missing_path(bindings_dir.join("node"), create_dir)?.apply(|path| {
            missing_path_else(
                path.join("index.js"),
                allow_update,
                |path| generate_file(path, INDEX_JS_TEMPLATE, language_name, &generate_opts),
                |path| {
                    let index_js =
                        fs::read_to_string(path).with_context(|| "Failed to read index.js")?;
                    if index_js.contains("../../build/Release") {
                        eprintln!("Replacing index.js with new binding API");
                        generate_file(path, INDEX_JS_TEMPLATE, language_name, &generate_opts)?;
                    }
                    Ok(())
                },
            )?;

            missing_path(path.join("index.d.ts"), |path| {
                generate_file(path, INDEX_D_TS_TEMPLATE, language_name, &generate_opts)
            })?;

            missing_path(path.join("binding_test.js"), |path| {
                generate_file(
                    path,
                    BINDING_TEST_JS_TEMPLATE,
                    language_name,
                    &generate_opts,
                )
            })?;

            missing_path_else(
                path.join("binding.cc"),
                allow_update,
                |path| generate_file(path, JS_BINDING_CC_TEMPLATE, language_name, &generate_opts),
                |path| {
                    let binding_cc =
                        fs::read_to_string(path).with_context(|| "Failed to read binding.cc")?;
                    if binding_cc.contains("NAN_METHOD(New) {}") {
                        eprintln!("Replacing binding.cc with new binding API");
                        generate_file(path, JS_BINDING_CC_TEMPLATE, language_name, &generate_opts)?;
                    }
                    Ok(())
                },
            )?;

            // Create binding.gyp, or update it with new binding API.
            missing_path_else(
                repo_path.join("binding.gyp"),
                allow_update,
                |path| generate_file(path, BINDING_GYP_TEMPLATE, language_name, &generate_opts),
                |path| {
                    let binding_gyp =
                        fs::read_to_string(path).with_context(|| "Failed to read binding.gyp")?;
                    if binding_gyp.contains("require('nan')") {
                        eprintln!("Replacing binding.gyp with new binding API");
                        generate_file(path, BINDING_GYP_TEMPLATE, language_name, &generate_opts)?;
                    }
                    Ok(())
                },
            )?;

            Ok(())
        })?;
    }

    // Generate C bindings
    if tree_sitter_config.bindings.c {
        missing_path(bindings_dir.join("c"), create_dir)?.apply(|path| {
            missing_path(
                path.join(format!("tree-sitter-{language_name}.h")),
                |path| generate_file(path, PARSER_NAME_H_TEMPLATE, language_name, &generate_opts),
            )?;

            missing_path(
                path.join(format!("tree-sitter-{language_name}.pc.in")),
                |path| {
                    generate_file(
                        path,
                        PARSER_NAME_PC_IN_TEMPLATE,
                        language_name,
                        &generate_opts,
                    )
                },
            )?;

            missing_path(repo_path.join("Makefile"), |path| {
                generate_file(path, MAKEFILE_TEMPLATE, language_name, &generate_opts)
            })?;

            missing_path(repo_path.join("CMakeLists.txt"), |path| {
                generate_file(path, CMAKELISTS_TXT_TEMPLATE, language_name, &generate_opts)
            })?;

            Ok(())
        })?;
    }

    // Generate Go bindings
    if tree_sitter_config.bindings.go {
        missing_path(bindings_dir.join("go"), create_dir)?.apply(|path| {
            missing_path(path.join("binding.go"), |path| {
                generate_file(path, BINDING_GO_TEMPLATE, language_name, &generate_opts)
            })?;

            missing_path_else(
                path.join("binding_test.go"),
                allow_update,
                |path| {
                    generate_file(
                        path,
                        BINDING_TEST_GO_TEMPLATE,
                        language_name,
                        &generate_opts,
                    )
                },
                |path| {
                    let binding_test_go = fs::read_to_string(path)
                        .with_context(|| "Failed to read binding_test.go")?;
                    if binding_test_go.contains("smacker") {
                        eprintln!("Replacing binding_test.go with new binding API");
                        generate_file(
                            path,
                            BINDING_TEST_GO_TEMPLATE,
                            language_name,
                            &generate_opts,
                        )?;
                    }
                    Ok(())
                },
            )?;

            // Delete the old go.mod file that lives inside bindings/go, it now lives in the root
            // dir
            let go_mod_path = path.join("go.mod");
            if allow_update && go_mod_path.exists() {
                fs::remove_file(go_mod_path).with_context(|| "Failed to remove old go.mod file")?;
            }

            missing_path(repo_path.join("go.mod"), |path| {
                generate_file(path, GO_MOD_TEMPLATE, language_name, &generate_opts)
            })?;

            Ok(())
        })?;
    }

    // Generate Python bindings
    if tree_sitter_config.bindings.python {
        missing_path(bindings_dir.join("python"), create_dir)?.apply(|path| {
            let lang_path = path.join(format!("tree_sitter_{}", language_name.to_snake_case()));
            missing_path(&lang_path, create_dir)?;

            missing_path_else(
                lang_path.join("binding.c"),
                allow_update,
                |path| generate_file(path, PY_BINDING_C_TEMPLATE, language_name, &generate_opts),
                |path| {
                    let binding_c = fs::read_to_string(path)
                        .with_context(|| "Failed to read bindings/python/binding.c")?;
                    if !binding_c.contains("PyCapsule_New") {
                        eprintln!("Replacing bindings/python/binding.c with new binding API");
                        generate_file(path, PY_BINDING_C_TEMPLATE, language_name, &generate_opts)?;
                    }
                    Ok(())
                },
            )?;

            missing_path(lang_path.join("__init__.py"), |path| {
                generate_file(path, INIT_PY_TEMPLATE, language_name, &generate_opts)
            })?;

            missing_path(lang_path.join("__init__.pyi"), |path| {
                generate_file(path, INIT_PYI_TEMPLATE, language_name, &generate_opts)
            })?;

            missing_path(lang_path.join("py.typed"), |path| {
                generate_file(path, "", language_name, &generate_opts) // py.typed is empty
            })?;

            missing_path(path.join("tests"), create_dir)?.apply(|path| {
                missing_path(path.join("test_binding.py"), |path| {
                    generate_file(
                        path,
                        TEST_BINDING_PY_TEMPLATE,
                        language_name,
                        &generate_opts,
                    )
                })?;
                Ok(())
            })?;

            missing_path(repo_path.join("setup.py"), |path| {
                generate_file(path, SETUP_PY_TEMPLATE, language_name, &generate_opts)
            })?;

            missing_path(repo_path.join("pyproject.toml"), |path| {
                generate_file(
                    path,
                    PYPROJECT_TOML_TEMPLATE,
                    dashed_language_name.as_str(),
                    &generate_opts,
                )
            })?;

            Ok(())
        })?;
    }

    // Generate Swift bindings
    if tree_sitter_config.bindings.swift {
        missing_path(bindings_dir.join("swift"), create_dir)?.apply(|path| {
            let lang_path = path.join(format!("TreeSitter{}", language_name.to_upper_camel_case()));
            missing_path(&lang_path, create_dir)?;

            missing_path(lang_path.join(format!("{language_name}.h")), |path| {
                generate_file(path, PARSER_NAME_H_TEMPLATE, language_name, &generate_opts)
            })?;

            missing_path(
                path.join(format!(
                    "TreeSitter{}Tests",
                    language_name.to_upper_camel_case()
                )),
                create_dir,
            )?
            .apply(|path| {
                missing_path(
                    path.join(format!(
                        "TreeSitter{}Tests.swift",
                        language_name.to_upper_camel_case()
                    )),
                    |path| generate_file(path, TESTS_SWIFT_TEMPLATE, language_name, &generate_opts),
                )?;

                Ok(())
            })?;

            missing_path(repo_path.join("Package.swift"), |path| {
                generate_file(path, PACKAGE_SWIFT_TEMPLATE, language_name, &generate_opts)
            })?;

            Ok(())
        })?;
    }

    Ok(())
}

pub fn lookup_package_json_for_path(path: &Path) -> Result<(PathBuf, PackageJSON)> {
    let mut pathbuf = path.to_owned();
    loop {
        let package_json = pathbuf
            .exists()
            .then(|| -> Result<PackageJSON> {
                let file =
                    File::open(pathbuf.as_path()).with_context(|| "Failed to open package.json")?;
                serde_json::from_reader(BufReader::new(file)).context(
                    "Failed to parse package.json, is the `tree-sitter` section malformed?",
                )
            })
            .transpose()?;
        if let Some(package_json) = package_json {
            if package_json.tree_sitter.is_some() {
                return Ok((pathbuf, package_json));
            }
        }
        pathbuf.pop(); // package.json
        if !pathbuf.pop() {
            return Err(anyhow!(concat!(
                "Failed to locate a package.json file that has a \"tree-sitter\" section,",
                " please ensure you have one, and if you don't then consult the docs",
            )));
        }
        pathbuf.push("package.json");
    }
}

fn generate_file(
    path: &Path,
    template: &str,
    language_name: &str,
    generate_opts: &GenerateOpts,
) -> Result<()> {
    let filename = path.file_name().unwrap().to_str().unwrap();

    let mut replacement = template
        .replace(
            CAMEL_PARSER_NAME_PLACEHOLDER,
            &language_name.to_upper_camel_case(),
        )
        .replace(
            UPPER_PARSER_NAME_PLACEHOLDER,
            &language_name.to_shouty_snake_case(),
        )
        .replace(
            LOWER_PARSER_NAME_PLACEHOLDER,
            &language_name.to_snake_case(),
        )
        .replace(PARSER_NAME_PLACEHOLDER, language_name)
        .replace(CLI_VERSION_PLACEHOLDER, CLI_VERSION)
        .replace(RUST_BINDING_VERSION_PLACEHOLDER, RUST_BINDING_VERSION)
        .replace(ABI_VERSION_MAX_PLACEHOLDER, &ABI_VERSION_MAX.to_string())
        .replace(
            PARSER_VERSION_PLACEHOLDER,
            &generate_opts.version.to_string(),
        );

    if let Some(name) = generate_opts.author_name {
        replacement = replacement.replace(AUTHOR_NAME_PLACEHOLDER, name);
    } else {
        match filename {
            "package.json" => {
                replacement = replacement.replace(AUTHOR_NAME_PLACEHOLDER_JS, "");
            }
            "pyproject.toml" => {
                replacement = replacement.replace(AUTHOR_NAME_PLACEHOLDER_PY, "");
            }
            "grammar.js" => {
                replacement = replacement.replace(AUTHOR_NAME_PLACEHOLDER_GRAMMAR, "");
            }
            "Cargo.toml" => {
                replacement = replacement.replace(AUTHOR_NAME_PLACEHOLDER_RS, "");
            }
            _ => {}
        }
    }

    if let Some(email) = generate_opts.author_email {
        replacement = replacement.replace(AUTHOR_EMAIL_PLACEHOLDER, email);
    } else {
        match filename {
            "package.json" => {
                replacement = replacement.replace(AUTHOR_EMAIL_PLACEHOLDER_JS, "");
            }
            "pyproject.toml" => {
                replacement = replacement.replace(AUTHOR_EMAIL_PLACEHOLDER_PY, "");
            }
            "grammar.js" => {
                replacement = replacement.replace(AUTHOR_EMAIL_PLACEHOLDER_GRAMMAR, "");
            }
            "Cargo.toml" => {
                replacement = replacement.replace(AUTHOR_EMAIL_PLACEHOLDER_RS, "");
            }
            _ => {}
        }
    }

    if filename == "package.json" {
        if let Some(url) = generate_opts.author_url {
            replacement = replacement.replace(AUTHOR_URL_PLACEHOLDER, url);
        } else {
            replacement = replacement.replace(AUTHOR_URL_PLACEHOLDER_JS, "");
        }
    }

    if generate_opts.author_name.is_none()
        && generate_opts.author_email.is_none()
        && generate_opts.author_url.is_none()
        && filename == "package.json"
    {
        if let Some(start_idx) = replacement.find(AUTHOR_BLOCK_JS) {
            if let Some(end_idx) = replacement[start_idx..]
                .find("},")
                .map(|i| i + start_idx + 2)
            {
                replacement.replace_range(start_idx..end_idx, "");
            }
        }
    } else if generate_opts.author_name.is_none() && generate_opts.author_email.is_none() {
        match filename {
            "pyproject.toml" => {
                if let Some(start_idx) = replacement.find(AUTHOR_BLOCK_PY) {
                    if let Some(end_idx) = replacement[start_idx..]
                        .find("}]")
                        .map(|i| i + start_idx + 2)
                    {
                        replacement.replace_range(start_idx..end_idx, "");
                    } else {
                        println!("none 2");
                    }
                } else {
                    println!("none 1");
                }
            }
            "grammar.js" => {
                if let Some(start_idx) = replacement.find(AUTHOR_BLOCK_GRAMMAR) {
                    if let Some(end_idx) = replacement[start_idx..]
                        .find(" \n")
                        .map(|i| i + start_idx + 1)
                    {
                        replacement.replace_range(start_idx..end_idx, "");
                    } else {
                        println!("none 2");
                    }
                } else {
                    println!("none 1");
                }
            }
            "Cargo.toml" => {
                if let Some(start_idx) = replacement.find(AUTHOR_BLOCK_RS) {
                    if let Some(end_idx) = replacement[start_idx..]
                        .find("\"]")
                        .map(|i| i + start_idx + 2)
                    {
                        replacement.replace_range(start_idx..end_idx, "");
                    }
                }
            }
            _ => {}
        }
    }

    match generate_opts.license {
        Some(license) => replacement = replacement.replace(PARSER_LICENSE_PLACEHOLDER, license),
        _ => replacement = replacement.replace(PARSER_LICENSE_PLACEHOLDER, "MIT"),
    }

    match generate_opts.description {
        Some(description) => {
            replacement = replacement.replace(PARSER_DESCRIPTION_PLACEHOLDER, description)
        }
        _ => {
            replacement = replacement.replace(
                PARSER_DESCRIPTION_PLACEHOLDER,
                &format!(
                    "{} grammar for tree-sitter",
                    language_name.to_upper_camel_case()
                ),
            )
        }
    }

    match generate_opts.repository {
        Some(repository) => {
            replacement = replacement
                .replace(
                    PARSER_URL_STRIPPED_PLACEHOLDER,
                    &repository.replace("https://", "").to_lowercase(),
                )
                .replace(PARSER_URL_PLACEHOLDER, &repository.to_lowercase())
        }
        _ => {
            replacement = replacement
                .replace(
                    PARSER_URL_STRIPPED_PLACEHOLDER,
                    &format!(
                        "github.com/tree-sitter/tree-sitter-{}",
                        language_name.to_lowercase()
                    ),
                )
                .replace(
                    PARSER_URL_PLACEHOLDER,
                    &format!(
                        "https://github.com/tree-sitter/tree-sitter-{}",
                        language_name.to_lowercase()
                    ),
                )
        }
    }

    write_file(path, replacement)
}

fn create_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("Failed to create {:?}", path.to_string_lossy()))
}

#[derive(PartialEq, Eq, Debug)]
enum PathState<P>
where
    P: AsRef<Path>,
{
    Exists(P),
    Missing(P),
}

#[allow(dead_code)]
impl<P> PathState<P>
where
    P: AsRef<Path>,
{
    fn exists(&self, mut action: impl FnMut(&Path) -> Result<()>) -> Result<&Self> {
        if let Self::Exists(path) = self {
            action(path.as_ref())?;
        }
        Ok(self)
    }

    fn missing(&self, mut action: impl FnMut(&Path) -> Result<()>) -> Result<&Self> {
        if let Self::Missing(path) = self {
            action(path.as_ref())?;
        }
        Ok(self)
    }

    fn apply(&self, mut action: impl FnMut(&Path) -> Result<()>) -> Result<&Self> {
        action(self.as_path())?;
        Ok(self)
    }

    fn apply_state(&self, mut action: impl FnMut(&Self) -> Result<()>) -> Result<&Self> {
        action(self)?;
        Ok(self)
    }

    fn as_path(&self) -> &Path {
        match self {
            Self::Exists(path) | Self::Missing(path) => path.as_ref(),
        }
    }
}

fn missing_path<P, F>(path: P, mut action: F) -> Result<PathState<P>>
where
    P: AsRef<Path>,
    F: FnMut(&Path) -> Result<()>,
{
    let path_ref = path.as_ref();
    if !path_ref.exists() {
        action(path_ref)?;
        Ok(PathState::Missing(path))
    } else {
        Ok(PathState::Exists(path))
    }
}

fn missing_path_else<P, T, F>(
    path: P,
    allow_update: bool,
    mut action: T,
    mut else_action: F,
) -> Result<PathState<P>>
where
    P: AsRef<Path>,
    T: FnMut(&Path) -> Result<()>,
    F: FnMut(&Path) -> Result<()>,
{
    let path_ref = path.as_ref();
    if !path_ref.exists() {
        action(path_ref)?;
        Ok(PathState::Missing(path))
    } else {
        if allow_update {
            else_action(path_ref)?;
        }
        Ok(PathState::Exists(path))
    }
}
