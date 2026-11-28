//! The examples at the top of every `--help` (spec §11.1): 2 to 4 real command lines each, with
//! a `--json` one, and a `hali --dry-run …` one for every command that can write. `--dry-run`
//! always comes straight after `hali`, the prefix form the skill pre-approves (spec §11.2).
//! Resources are named by path, never by UUID.

pub const HALI: &str = "Examples:
  hali describe --json
  hali find --tag computing --json
  hali --dry-run edit ~/notes/ideas.md --add-tag ml
  hali help agents";

pub const ADD: &str = "Examples:
  hali --dry-run add ~/papers/ldp.pdf --type Paper --tag ml
  hali add ~/notes/ideas.md ~/notes/todo.md --json";

pub const NEW: &str = "Examples:
  hali --dry-run new \"Reading list\" --type Note --tag ml
  hali --dry-run --json new \"Meeting notes\"
  hali --dry-run new \"Draft\" --path-only";

pub const CONCEPT: &str = "Examples:
  hali concept list --json
  hali --dry-run concept add \"linked data\" --broader computing";

pub const CONCEPT_ADD: &str = "Examples:
  hali --dry-run concept add \"linked data\" --broader computing
  hali --dry-run --json concept add databases";

pub const CONCEPT_SHOW: &str = "Examples:
  hali concept show ml --json
  hali concept show computing";

pub const CONCEPT_RENAME: &str = "Examples:
  hali --dry-run concept rename ml \"machine learning\"
  hali concept rename ml \"machine learning\" --json";

pub const CONCEPT_EDIT: &str = "Examples:
  hali --dry-run concept edit ml --add-alt \"machine learning\"
  hali concept edit ml --add-alt \"machine learning\" --json";

pub const CONCEPT_REMOVE: &str = "Examples:
  hali --dry-run concept remove drafts
  hali --dry-run --json concept remove drafts";

pub const CONCEPT_LIST: &str = "Examples:
  hali concept list --json
  hali concept list --limit 10 --offset 10";

pub const EDIT: &str = "Examples:
  hali --dry-run edit ~/notes/ideas.md --add-type Note --add-tag computing
  hali edit ~/papers/ldp.pdf --title \"Linked Data Platform\" --json";

pub const FORGET: &str = "Examples:
  hali --dry-run forget ~/notes/todo.md
  hali --dry-run --json forget ~/papers/ldp.pdf --force";

pub const REFRESH: &str = "Examples:
  hali --dry-run refresh --all
  hali refresh ~/papers/ldp.pdf --json";

pub const SCAN: &str = "Examples:
  hali --dry-run scan ~/papers --exclude \"*.tmp\"
  hali scan --list --json
  hali --dry-run --json scan --prune
  hali --dry-run scan";

pub const RELATE: &str = "Examples:
  hali --dry-run relate ~/notes/todo.md annotates ~/papers/ldp.pdf
  hali relate ~/notes/todo.md dcterms:references ~/papers/ldp.pdf --json";

pub const RELINK: &str = "Examples:
  hali --dry-run relink
  hali relink --root ~/archive --json
  hali relink ~/notes/old.pdf ~/archive/old.pdf --json";

pub const DESCRIBE: &str = "Examples:
  hali describe --json
  hali describe --brief";

pub const QUERY: &str = "Examples:
  hali query \"SELECT ?title WHERE { ?r a hali:Resource ; dcterms:title ?title } LIMIT 10\"
  hali query \"ASK { ?r a hali:Paper }\" --json";

pub const RELATION: &str = "Examples:
  hali relation list --json
  hali --dry-run relation add cites --parent dcterms:references";

pub const RELATION_ADD: &str = "Examples:
  hali --dry-run relation add cites --parent dcterms:references
  hali --dry-run --json relation add summarizes --parent dcterms:relation";

pub const RELATION_LIST: &str = "Examples:
  hali relation list --json
  hali relation list --limit 10";

pub const RELATION_REMOVE: &str = "Examples:
  hali --dry-run relation remove supersedes
  hali --dry-run --json relation remove supersedes";

pub const UNRELATE: &str = "Examples:
  hali --dry-run unrelate ~/notes/ideas.md annotates ~/papers/ldp.pdf
  hali unrelate ~/notes/ideas.md annotates ~/papers/ldp.pdf --json";

pub const SCHEMA: &str = "Examples:
  hali schema --json
  hali schema find
  hali schema error";

pub const FIND: &str = "Examples:
  hali find --tag computing --json
  hali find --missing --path-only
  hali find --related-to ~/papers/ldp.pdf --via annotates --json
  hali find --type Paper --sort title --limit 10";

pub const TYPE: &str = "Examples:
  hali type list --json
  hali --dry-run type add Article --parent Paper";

pub const TYPE_ADD: &str = "Examples:
  hali --dry-run type add Article --parent Paper
  hali --dry-run --json type add Book";

pub const TYPE_LIST: &str = "Examples:
  hali type list --json
  hali type list --limit 10";

pub const TYPE_REMOVE: &str = "Examples:
  hali --dry-run type remove Note
  hali --dry-run --json type remove Note";

pub const SHOW: &str = "Examples:
  hali show ~/papers/ldp.pdf --json
  hali show ~/papers/ldp.pdf --path-only";

pub const VALIDATE: &str = "Examples:
  hali validate --json
  hali validate --limit 10";

pub const SKILL: &str = "Examples:
  hali skill install --json
  hali --dry-run skill install --agents";

pub const SKILL_INSTALL: &str = "Examples:
  hali --dry-run --json skill install --agents
  hali skill install
  hali skill install --print";
