use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
    #[serde(alias = "allowed-tools")]
    allowed_tools: Vec<String>,
    #[serde(alias = "user-invocable")]
    user_invocable: bool,
    #[serde(alias = "disable-model-invocation")]
    disable_model_invocation: bool,
    #[serde(default, alias = "requires-action", alias = "requiresAction")]
    requires_action: Option<bool>,
}

fn parse_skill(markdown: &str) -> (SkillFrontmatter, &str) {
    let rest = markdown
        .strip_prefix("---\n")
        .expect("skill starts with frontmatter");
    let (frontmatter, body) = rest
        .split_once("\n---\n")
        .expect("skill frontmatter terminates");
    let parsed = serde_yaml::from_str(frontmatter).expect("skill frontmatter parses");
    (parsed, body)
}

#[test]
fn image_generation_skill_guides_foreground_bash_helper_use() {
    let (frontmatter, body) = parse_skill(include_str!(
        "../../../resources/skills/image-generation/SKILL.md"
    ));

    assert_eq!(frontmatter.name, "image-generation");
    assert!(!frontmatter.description.contains("ImageGeneration"));
    assert_eq!(frontmatter.allowed_tools, vec!["Bash"]);
    assert!(frontmatter.user_invocable);
    assert!(!frontmatter.disable_model_invocation);
    assert_eq!(frontmatter.requires_action, Some(true));
    assert!(body.contains("foreground Bash"));
    assert!(body.contains("Progress-only or promise-only replies are not completion"));
    assert!(body.contains("explicit long Bash timeout"));
    assert!(body.contains("imagegen --prompt"));
    assert!(!body.contains("puffer internal-tool"));
    assert!(body.contains("--count"));
    assert!(body.contains("one logical request"));
    assert!(body.contains("prompt file paths"));
    assert!(body.contains("allowed-tools is guidance"));
    assert!(body.contains("Do not hand-author SVG"));
    // Per-call provider/model override (added to the CLI) must be documented.
    assert!(body.contains("--provider"));
    assert!(body.contains("--model"));
}

#[test]
fn video_generation_skill_guides_foreground_bash_helper_use() {
    let (frontmatter, body) = parse_skill(include_str!(
        "../../../resources/skills/video-generation/SKILL.md"
    ));

    assert_eq!(frontmatter.name, "video-generation");
    assert!(!frontmatter.description.contains("VideoGeneration"));
    assert_eq!(frontmatter.allowed_tools, vec!["Bash"]);
    assert!(frontmatter.user_invocable);
    assert!(!frontmatter.disable_model_invocation);
    assert_eq!(frontmatter.requires_action, Some(true));
    assert!(body.contains("foreground Bash"));
    assert!(body.contains("Progress-only or promise-only replies are not completion"));
    assert!(body.contains("explicit long Bash timeout"));
    assert!(body.contains("videogen --prompt"));
    assert!(!body.contains("puffer internal-tool"));
    assert!(body.contains("--parameters-json"));
    assert!(body.contains("--image-reference"));
    assert!(body.contains("https://"));
    assert!(body.contains("asset://"));
    assert!(body.contains("local paths"));
    assert!(body.contains("scalar"));
    assert!(body.contains("allowed-tools is guidance"));
    assert!(body.contains("persisted video artifact"));
    assert!(!body.contains("text-to-video only"));
    // Per-call provider/model override (added to the CLI) must be documented.
    assert!(body.contains("--provider"));
    assert!(body.contains("--model"));
}

#[test]
fn short_drama_skill_requires_action_after_activation() {
    let (frontmatter, body) = parse_skill(include_str!(
        "../../../resources/skills/short-drama-generation/SKILL.md"
    ));

    assert_eq!(frontmatter.name, "short-drama-generation");
    assert_eq!(frontmatter.requires_action, Some(true));
    assert!(frontmatter.allowed_tools.contains(&"Write".to_string()));
    assert!(body.contains("Progress-only or promise-only replies are not completion"));
}

#[test]
fn short_drama_skill_gates_model_selection_in_stage0() {
    let body = include_str!("../../../resources/skills/short-drama-generation/SKILL.md");
    // Stage 0 exists and uses the documented canvasId.
    assert!(body.contains("canvas-drama-<slug>-stage0"));
    // Stage 0 renders the self-contained node, not hand-built selects.
    assert!(body.contains("mediaModelSelect"));
    // The four read-back keys are documented.
    assert!(body.contains("imgProvider"));
    assert!(body.contains("imgModel"));
    assert!(body.contains("vidProvider"));
    assert!(body.contains("vidModel"));
    // The old bash flow and hand-built cascade are gone.
    assert!(!body.contains("media-capabilities"));
    assert!(!body.contains("dependentSelect"));
    // Settings prompt contract for a kind with no provider.
    assert!(body.to_lowercase().contains("settings"));
    // Per-call selection threads into generation.
    assert!(body.contains("--provider"));
    assert!(body.contains("--model"));
}

#[test]
fn short_drama_stage1_script_gate_is_a_bare_textarea() {
    let body = include_str!("../../../resources/skills/short-drama-generation/SKILL.md");
    // The script gate goes straight to the textarea: no summary, no wrapping card,
    // no Regenerate button — the canvas title is the only chrome.
    assert!(body.contains("{title:\"Script draft\",body:[{type:\"textarea\""));
}

#[test]
fn short_drama_id_is_session_scoped_for_fresh_directory() {
    let body = include_str!("../../../resources/skills/short-drama-generation/SKILL.md");
    // Each run must land in a fresh drama directory: <id> = <slug>-<session8>,
    // derived from the session id, so a re-run with a similar prompt never reuses
    // a prior run's directory (which makes the manifest write fail on an unread,
    // pre-existing file). The session suffix is the uniqueness guarantee.
    assert!(body.contains("<slug>-<session8>"));
    assert!(body.contains("session id"));
}

#[test]
fn short_drama_stage2_storyboard_draft_uses_card_layout() {
    let body = include_str!("../../../resources/skills/short-drama-generation/SKILL.md");
    // The storyboard renders the editableTable directly in `body` using the
    // card-per-shot layout so long subject/action/characters fields wrap instead
    // of scrolling — no wrapping card, no Regenerate button.
    assert!(body.contains("{title:\"Storyboard\",body:[{type:\"editableTable\""));
    assert!(body.contains("layout:\"cards\""));
}

#[test]
fn short_drama_stage3_generates_one_image_per_character() {
    let body = include_str!("../../../resources/skills/short-drama-generation/SKILL.md");
    // One image per character; combined sheets are forbidden.
    assert!(body.contains("one image per character"));
    assert!(body.contains("N characters → N calls → N images"));
    assert!(body.contains("Never combine multiple characters into one image"));
    // Stage 3 canvas: default-checked multi picker, one item per character, no wrapping card.
    assert!(body.contains("canvas-drama-<slug>-stage3"));
    assert!(body.contains("multi:true"));
    assert!(body.contains("{id,url,label,description}"));
    assert!(body.contains("no wrapping card"));
    assert!(body.contains("the character name only"));
    // Each character reference is persisted for Stage 4 per-shot lookup.
    assert!(body.contains("characterRefs"));
    // The old Regenerate toggle is removed.
    assert!(!body.contains("id:\"regen\""));
    assert!(!body.contains("values.regen"));
}
