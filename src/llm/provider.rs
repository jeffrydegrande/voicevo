use std::fmt;

use anyhow::{Context, Result};

/// Supported LLM providers.
#[derive(Debug, Clone)]
pub enum Provider {
    Anthropic,
    OpenAI,
}

/// Model tier — controls the speed/capability tradeoff.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModelTier {
    /// Fastest, cheapest: Haiku / GPT-5.2
    Fast,
    /// Balanced default: Sonnet / GPT-5.2
    Default,
    /// Most capable: Opus / GPT-5.2-pro
    Think,
}

impl Provider {
    /// Parse from a CLI string like "claude" or "gpt".
    pub fn from_str_loose(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "claude" | "anthropic" => Ok(Provider::Anthropic),
            "gpt" | "openai" => Ok(Provider::OpenAI),
            _ => anyhow::bail!(
                "Unknown provider: {s}. Use 'claude' or 'gpt'."
            ),
        }
    }

    /// The environment variable name for this provider's API key.
    pub fn api_key_env(&self) -> &'static str {
        match self {
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::OpenAI => "OPENAI_API_KEY",
        }
    }

    /// Read the API key from the environment.
    pub fn api_key(&self) -> Result<String> {
        let var = self.api_key_env();
        std::env::var(var).with_context(|| {
            format!(
                "{var} not set. Export it in your shell:\n  export {var}=sk-..."
            )
        })
    }

    /// Default model (balanced tier).
    pub fn default_model(&self) -> &'static str {
        self.model_for_tier(ModelTier::Default)
    }

    /// Model ID for a given tier.
    pub fn model_for_tier(&self, tier: ModelTier) -> &'static str {
        match (self, tier) {
            (Provider::Anthropic, ModelTier::Fast)    => "claude-haiku-4-5-20251001",
            (Provider::Anthropic, ModelTier::Default) => "claude-sonnet-4-5-20250929",
            (Provider::Anthropic, ModelTier::Think)   => "claude-opus-4-6",

            (Provider::OpenAI, ModelTier::Fast)       => "gpt-5.2",
            (Provider::OpenAI, ModelTier::Default)    => "gpt-5.2",
            (Provider::OpenAI, ModelTier::Think)      => "gpt-5.2-pro",
        }
    }
}

impl ModelTier {
    /// Resolve from CLI flags. --model overrides everything.
    pub fn from_flags(fast: bool, think: bool) -> Self {
        match (fast, think) {
            (true, _) => ModelTier::Fast,
            (_, true) => ModelTier::Think,
            _ => ModelTier::Default,
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Provider::Anthropic => write!(f, "Claude"),
            Provider::OpenAI => write!(f, "GPT"),
        }
    }
}

impl fmt::Display for ModelTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelTier::Fast => write!(f, "fast"),
            ModelTier::Default => write!(f, "default"),
            ModelTier::Think => write!(f, "think"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_provider_aliases() {
        assert!(matches!(Provider::from_str_loose("claude").unwrap(), Provider::Anthropic));
        assert!(matches!(Provider::from_str_loose("anthropic").unwrap(), Provider::Anthropic));
        assert!(matches!(Provider::from_str_loose("gpt").unwrap(), Provider::OpenAI));
        assert!(matches!(Provider::from_str_loose("openai").unwrap(), Provider::OpenAI));
        assert!(matches!(Provider::from_str_loose("Claude").unwrap(), Provider::Anthropic));
    }

    #[test]
    fn parse_unknown_provider() {
        assert!(Provider::from_str_loose("gemini").is_err());
    }

    #[test]
    fn default_models() {
        let a = Provider::Anthropic;
        assert!(a.default_model().contains("sonnet"));
        let o = Provider::OpenAI;
        assert!(o.default_model().starts_with("gpt"));
    }

    #[test]
    fn tier_models() {
        let a = Provider::Anthropic;
        assert!(a.model_for_tier(ModelTier::Fast).contains("haiku"));
        assert!(a.model_for_tier(ModelTier::Default).contains("sonnet"));
        assert!(a.model_for_tier(ModelTier::Think).contains("opus"));

        let o = Provider::OpenAI;
        assert!(o.model_for_tier(ModelTier::Think).contains("pro"));
    }

    #[test]
    fn tier_from_flags() {
        assert_eq!(ModelTier::from_flags(false, false), ModelTier::Default);
        assert_eq!(ModelTier::from_flags(true, false), ModelTier::Fast);
        assert_eq!(ModelTier::from_flags(false, true), ModelTier::Think);
    }
}
