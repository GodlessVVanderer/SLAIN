// SLAIN LEGAL EVIDENCE PACKAGE GENERATOR
// Creates court-ready evidence packages from FORUMYZE data
// Integrates with CourtListener for case law research
// Target customers: Law firms handling civil rights, wrongful death, police misconduct

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::forumyze::{Discussion, ForumyzeResult, YouTubeComment};

// ============================================================================
// EVIDENCE PACKAGE TYPES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePackage {
    /// Unique package ID
    pub id: String,
    /// Case reference (law firm's internal case number)
    pub case_reference: Option<String>,
    /// Video evidence source
    pub video_evidence: VideoEvidence,
    /// Witness testimonies extracted from comments
    pub witness_statements: Vec<WitnessStatement>,
    /// Thematic analysis of public sentiment
    pub thematic_analysis: ThematicAnalysis,
    /// Related case law from CourtListener
    pub case_law: Vec<CaseLawReference>,
    /// Timeline of events from comments
    pub timeline: Vec<TimelineEvent>,
    /// Statistical summary
    pub statistics: EvidenceStatistics,
    /// Package metadata
    pub metadata: PackageMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoEvidence {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub url: String,
    pub published_at: String,
    pub view_count: u64,
    pub comment_count: u64,
    pub duration_seconds: u64,
    /// Archive URLs (Internet Archive, etc.)
    pub archive_urls: Vec<String>,
    /// SHA-256 hash of video file if downloaded
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessStatement {
    /// Unique ID
    pub id: String,
    /// Original comment ID
    pub comment_id: String,
    /// Author (anonymized option available)
    pub author: String,
    pub author_channel_url: String,
    /// Statement text
    pub statement: String,
    /// Timestamp
    pub timestamp: String,
    /// Engagement metrics (credibility indicator)
    pub like_count: u64,
    pub reply_count: u64,
    /// Classification
    pub classification: StatementClassification,
    /// Extracted claims
    pub claims: Vec<ExtractedClaim>,
    /// Sentiment (-1.0 to 1.0)
    pub sentiment: f32,
    /// Relevance score (0.0 to 1.0)
    pub relevance_score: f32,
    /// Is this a first-hand account?
    pub first_hand: bool,
    /// Geographic indicators mentioned
    pub locations_mentioned: Vec<String>,
    /// Dates/times mentioned
    pub temporal_references: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StatementClassification {
    /// Direct eyewitness account
    Eyewitness,
    /// Claims personal knowledge
    FirstHand,
    /// Reporting what others said
    SecondHand,
    /// Expert commentary (legal, medical, etc.)
    Expert,
    /// General opinion
    Opinion,
    /// Question seeking information
    Question,
    /// Emotional response
    Emotional,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedClaim {
    pub claim_text: String,
    pub claim_type: ClaimType,
    pub confidence: f32,
    pub supporting_context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClaimType {
    /// "I saw X happen"
    Observation,
    /// "X occurred at Y time"
    Temporal,
    /// "This happened at Z location"  
    Location,
    /// "Person A did B"
    Action,
    /// "The victim/officer was C"
    Identity,
    /// "There were N people"
    Quantity,
    /// "X caused Y"
    Causation,
    /// Contradicts official narrative
    Contradiction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThematicAnalysis {
    /// Major themes identified
    pub themes: Vec<Theme>,
    /// Public sentiment breakdown
    pub sentiment_breakdown: SentimentBreakdown,
    /// Key narratives
    pub narratives: Vec<Narrative>,
    /// Disputed facts
    pub disputed_facts: Vec<DisputedFact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub description: String,
    pub comment_count: usize,
    pub representative_quotes: Vec<String>,
    pub sentiment: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentBreakdown {
    pub positive: f32,
    pub negative: f32,
    pub neutral: f32,
    pub outrage: f32,
    pub sympathy: f32,
    pub calls_for_justice: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Narrative {
    pub id: String,
    pub summary: String,
    pub supporting_statements: Vec<String>,
    pub contradicting_statements: Vec<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisputedFact {
    pub fact: String,
    pub claim_a: String,
    pub claim_b: String,
    pub comment_ids_a: Vec<String>,
    pub comment_ids_b: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseLawReference {
    pub case_id: String,
    pub case_name: String,
    pub citation: String,
    pub court: String,
    pub date_filed: String,
    pub relevance: String,
    pub key_holdings: Vec<String>,
    pub url: String,
    pub cite_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub timestamp: String,
    pub description: String,
    pub source_type: TimelineSourceType,
    pub source_ids: Vec<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimelineSourceType {
    VideoContent,
    WitnessComment,
    NewsReport,
    OfficialStatement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceStatistics {
    pub total_comments_analyzed: usize,
    pub witness_statements_extracted: usize,
    pub first_hand_accounts: usize,
    pub unique_authors: usize,
    pub date_range: Option<(String, String)>,
    pub avg_engagement: f32,
    pub themes_identified: usize,
    pub claims_extracted: usize,
    pub case_law_references: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    pub generated_at: String,
    pub generator_version: String,
    pub forumyze_version: String,
    pub export_formats: Vec<String>,
    pub hash: String,
}

// ============================================================================
// COURTLISTENER CLIENT
// ============================================================================

pub struct CourtListenerClient {
    api_token: Option<String>,
    client: reqwest::Client,
    base_url: String,
}

impl CourtListenerClient {
    pub fn new(api_token: Option<String>) -> Self {
        Self {
            api_token,
            client: reqwest::Client::new(),
            base_url: "https://www.courtlistener.com/api/rest/v4".to_string(),
        }
    }

    /// Search case law opinions
    pub async fn search_case_law(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CaseLawReference>, String> {
        let url = format!(
            "{}/search/?q={}&type=o",
            self.base_url,
            urlencoding::encode(query)
        );

        let mut req = self.client.get(&url);
        if let Some(token) = &self.api_token {
            req = req.header("Authorization", format!("Token {}", token));
        }

        let response = req
            .send()
            .await
            .map_err(|e| format!("CourtListener request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("CourtListener API error: {}", response.status()));
        }

        #[derive(Deserialize)]
        struct SearchResponse {
            count: u32,
            results: Vec<CaseResult>,
        }

        #[derive(Deserialize)]
        struct CaseResult {
            cluster_id: Option<u64>,
            #[serde(rename = "caseName")]
            case_name: Option<String>,
            court: Option<String>,
            #[serde(rename = "dateFiled")]
            date_filed: Option<String>,
            citation: Option<Vec<String>>,
            absolute_url: Option<String>,
            #[serde(rename = "citeCount")]
            cite_count: Option<u32>,
            status: Option<String>,
        }

        let data: SearchResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse CourtListener response: {}", e))?;

        let cases: Vec<CaseLawReference> = data
            .results
            .into_iter()
            .take(limit)
            .filter_map(|r| {
                Some(CaseLawReference {
                    case_id: r.cluster_id?.to_string(),
                    case_name: r.case_name?,
                    citation: r.citation.unwrap_or_default().join(", "),
                    court: r.court.unwrap_or_default(),
                    date_filed: r.date_filed.unwrap_or_default(),
                    relevance: String::new(),
                    key_holdings: vec![],
                    url: format!(
                        "https://www.courtlistener.com{}",
                        r.absolute_url.unwrap_or_default()
                    ),
                    cite_count: r.cite_count.unwrap_or(0),
                })
            })
            .collect();

        Ok(cases)
    }

    /// Build evidence package case law from themes
    pub async fn find_relevant_case_law(&self, themes: &[String]) -> Vec<CaseLawReference> {
        let mut all_cases = Vec::new();
        let mut seen_ids = HashSet::new();

        for theme in themes {
            // Generate legal search queries
            let queries = self.build_legal_queries(theme);

            for query in queries {
                if let Ok(cases) = self.search_case_law(&query, 5).await {
                    for case in cases {
                        if !seen_ids.contains(&case.case_id) {
                            seen_ids.insert(case.case_id.clone());
                            all_cases.push(case);
                        }
                    }
                }

                // Rate limiting
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }

        // Sort by citation count (most cited = most relevant)
        all_cases.sort_by(|a, b| b.cite_count.cmp(&a.cite_count));
        all_cases.truncate(50);

        all_cases
    }

    fn build_legal_queries(&self, theme: &str) -> Vec<String> {
        let theme_lower = theme.to_lowercase();
        let mut queries = vec![theme.to_string()];

        // Add legal context based on keywords
        if theme_lower.contains("police") || theme_lower.contains("officer") {
            queries.push(format!("{} excessive force", theme));
            queries.push(format!("{} qualified immunity", theme));
            queries.push(format!("{} section 1983", theme));
        }

        if theme_lower.contains("death")
            || theme_lower.contains("killed")
            || theme_lower.contains("shot")
        {
            queries.push(format!("{} wrongful death", theme));
            queries.push(format!("{} use of force", theme));
        }

        if theme_lower.contains("arrest") {
            queries.push(format!("{} false arrest", theme));
            queries.push(format!("{} fourth amendment", theme));
        }

        if theme_lower.contains("choke")
            || theme_lower.contains("restrain")
            || theme_lower.contains("knee")
        {
            queries.push(format!("{} positional asphyxia", theme));
        }

        queries.truncate(5);
        queries
    }
}

// ============================================================================
// EVIDENCE PACKAGE BUILDER
// ============================================================================

pub struct EvidencePackageBuilder {
    courtlistener: CourtListenerClient,
}

impl EvidencePackageBuilder {
    pub fn new(courtlistener_token: Option<String>) -> Self {
        Self {
            courtlistener: CourtListenerClient::new(courtlistener_token),
        }
    }

    /// Build evidence package from FORUMYZE results
    pub async fn build(
        &self,
        forumyze: &ForumyzeResult,
        case_reference: Option<String>,
    ) -> EvidencePackage {
        let package_id = uuid::Uuid::new_v4().to_string();

        // Extract witness statements
        let witness_statements = self.extract_witness_statements(forumyze);

        // Build thematic analysis
        let thematic_analysis = self.build_thematic_analysis(forumyze);

        // Find relevant case law
        let themes: Vec<String> = forumyze.topics.clone();
        let case_law = self.courtlistener.find_relevant_case_law(&themes).await;

        // Build timeline
        let timeline = self.extract_timeline(&witness_statements);

        // Calculate statistics
        let statistics = EvidenceStatistics {
            total_comments_analyzed: forumyze.total_comments,
            witness_statements_extracted: witness_statements.len(),
            first_hand_accounts: witness_statements.iter().filter(|w| w.first_hand).count(),
            unique_authors: self.count_unique_authors(forumyze),
            date_range: self.get_date_range(&witness_statements),
            avg_engagement: self.calc_avg_engagement(forumyze),
            themes_identified: thematic_analysis.themes.len(),
            claims_extracted: witness_statements.iter().map(|w| w.claims.len()).sum(),
            case_law_references: case_law.len(),
        };

        // Build video evidence
        let video_evidence = VideoEvidence {
            video_id: forumyze.video.video_id.clone(),
            title: forumyze.video.title.clone(),
            channel: forumyze.video.channel_name.clone(),
            url: format!(
                "https://www.youtube.com/watch?v={}",
                forumyze.video.video_id
            ),
            published_at: forumyze.video.published_at.clone(),
            view_count: forumyze.video.view_count,
            comment_count: forumyze.video.comment_count,
            duration_seconds: forumyze.video.duration_seconds,
            archive_urls: vec![format!(
                "https://web.archive.org/web/https://www.youtube.com/watch?v={}",
                forumyze.video.video_id
            )],
            content_hash: None,
        };

        let metadata = PackageMetadata {
            generated_at: Utc::now().to_rfc3339(),
            generator_version: "1.0.0".to_string(),
            forumyze_version: crate::VERSION.to_string(),
            export_formats: vec!["json".to_string(), "pdf".to_string(), "docx".to_string()],
            hash: self.calculate_package_hash(&package_id),
        };

        EvidencePackage {
            id: package_id,
            case_reference,
            video_evidence,
            witness_statements,
            thematic_analysis,
            case_law,
            timeline,
            statistics,
            metadata,
        }
    }

    /// Extract witness statements from comments
    fn extract_witness_statements(&self, forumyze: &ForumyzeResult) -> Vec<WitnessStatement> {
        let mut statements = Vec::new();

        // Process core dialogue (most substantive)
        for comment in &forumyze.core_dialogue {
            if let Some(stmt) = self.comment_to_witness_statement(comment, 0.8) {
                statements.push(stmt);
            }
        }

        // Process anomalies (controversial/unique perspectives)
        for comment in &forumyze.anomalies {
            if let Some(stmt) = self.comment_to_witness_statement(comment, 0.7) {
                statements.push(stmt);
            }
        }

        // Process questions (may reveal information)
        for comment in &forumyze.questions {
            if let Some(stmt) = self.comment_to_witness_statement(comment, 0.5) {
                statements.push(stmt);
            }
        }

        // Sort by relevance score
        statements.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap());

        statements
    }

    fn comment_to_witness_statement(
        &self,
        comment: &YouTubeComment,
        base_relevance: f32,
    ) -> Option<WitnessStatement> {
        let text = &comment.text;

        // Classify the statement
        let classification = self.classify_statement(text);

        // Extract claims
        let claims = self.extract_claims(text);

        // Detect first-hand account
        let first_hand = self.is_first_hand(text);

        // Calculate sentiment
        let sentiment = self.analyze_sentiment(text);

        // Extract locations and temporal references
        let locations = self.extract_locations(text);
        let temporal = self.extract_temporal_refs(text);

        // Calculate final relevance
        let relevance_score = base_relevance
            * (1.0 + (comment.like_count as f32).ln().max(0.0) * 0.1)
            * if first_hand { 1.5 } else { 1.0 }
            * if !claims.is_empty() { 1.3 } else { 1.0 };

        Some(WitnessStatement {
            id: uuid::Uuid::new_v4().to_string(),
            comment_id: comment.id.clone(),
            author: comment.author.clone(),
            author_channel_url: comment.author_channel.clone(),
            statement: text.clone(),
            timestamp: comment.published_at.clone(),
            like_count: comment.like_count,
            reply_count: comment.reply_count,
            classification,
            claims,
            sentiment,
            relevance_score: relevance_score.min(1.0),
            first_hand,
            locations_mentioned: locations,
            temporal_references: temporal,
        })
    }

    fn classify_statement(&self, text: &str) -> StatementClassification {
        let text_lower = text.to_lowercase();

        // Eyewitness indicators
        if text_lower.contains("i saw")
            || text_lower.contains("i witnessed")
            || text_lower.contains("i was there")
            || text_lower.contains("i watched it happen")
        {
            return StatementClassification::Eyewitness;
        }

        // First-hand indicators
        if text_lower.contains("i know")
            || text_lower.contains("i live")
            || text_lower.contains("my neighbor")
            || text_lower.contains("my friend")
            || text_lower.contains("i work")
        {
            return StatementClassification::FirstHand;
        }

        // Expert indicators
        if text_lower.contains("as a lawyer")
            || text_lower.contains("as a doctor")
            || text_lower.contains("as a nurse")
            || text_lower.contains("as a cop")
            || text_lower.contains("former officer")
            || text_lower.contains("legal expert")
        {
            return StatementClassification::Expert;
        }

        // Question
        if text.contains('?') && text.len() < 200 {
            return StatementClassification::Question;
        }

        // Second-hand
        if text_lower.contains("heard that")
            || text_lower.contains("someone said")
            || text_lower.contains("apparently")
            || text_lower.contains("reportedly")
        {
            return StatementClassification::SecondHand;
        }

        // Emotional
        let emotional_words = [
            "rip",
            "rest in peace",
            "so sad",
            "heartbreaking",
            "prayers",
            "justice",
        ];
        if emotional_words.iter().any(|w| text_lower.contains(w)) && text.len() < 100 {
            return StatementClassification::Emotional;
        }

        StatementClassification::Opinion
    }

    fn extract_claims(&self, text: &str) -> Vec<ExtractedClaim> {
        let mut claims = Vec::new();
        let text_lower = text.to_lowercase();

        // Observation claims
        let observation_patterns = [
            "i saw",
            "i watched",
            "i witnessed",
            "you can see",
            "the video shows",
            "clearly shows",
            "footage shows",
        ];
        for pattern in observation_patterns {
            if let Some(pos) = text_lower.find(pattern) {
                let context = self.extract_context(text, pos, 100);
                claims.push(ExtractedClaim {
                    claim_text: context.clone(),
                    claim_type: ClaimType::Observation,
                    confidence: 0.8,
                    supporting_context: context,
                });
            }
        }

        // Temporal claims
        let time_patterns = [
            "at ",
            "around ",
            "minutes before",
            "minutes after",
            "seconds",
            "o'clock",
            "am",
            "pm",
        ];
        for pattern in time_patterns {
            if text_lower.contains(pattern) {
                let context = self.extract_context(text, text_lower.find(pattern).unwrap(), 80);
                claims.push(ExtractedClaim {
                    claim_text: context.clone(),
                    claim_type: ClaimType::Temporal,
                    confidence: 0.6,
                    supporting_context: context,
                });
                break;
            }
        }

        // Contradiction claims
        let contradiction_patterns = [
            "but actually",
            "that's not true",
            "they're lying",
            "the truth is",
            "what really happened",
            "they left out",
            "didn't show",
        ];
        for pattern in contradiction_patterns {
            if let Some(pos) = text_lower.find(pattern) {
                let context = self.extract_context(text, pos, 120);
                claims.push(ExtractedClaim {
                    claim_text: context.clone(),
                    claim_type: ClaimType::Contradiction,
                    confidence: 0.7,
                    supporting_context: context,
                });
            }
        }

        claims
    }

    fn extract_context(&self, text: &str, pos: usize, max_len: usize) -> String {
        let start = pos.saturating_sub(20);
        let end = (pos + max_len).min(text.len());
        text[start..end].to_string()
    }

    fn is_first_hand(&self, text: &str) -> bool {
        let indicators = [
            "i saw",
            "i was there",
            "i witnessed",
            "i know him",
            "i know her",
            "i live",
            "my neighborhood",
            "my community",
            "i work",
        ];
        let text_lower = text.to_lowercase();
        indicators.iter().any(|i| text_lower.contains(i))
    }

    fn analyze_sentiment(&self, text: &str) -> f32 {
        let text_lower = text.to_lowercase();

        let positive = ["justice", "hero", "brave", "thank", "support", "peaceful"];
        let negative = [
            "murder",
            "killed",
            "wrong",
            "evil",
            "corrupt",
            "injustice",
            "disgusting",
        ];

        let pos_count = positive.iter().filter(|w| text_lower.contains(*w)).count() as f32;
        let neg_count = negative.iter().filter(|w| text_lower.contains(*w)).count() as f32;

        if pos_count + neg_count == 0.0 {
            return 0.0;
        }

        (pos_count - neg_count) / (pos_count + neg_count)
    }

    fn extract_locations(&self, text: &str) -> Vec<String> {
        // Simple extraction - in production would use NER
        let mut locations = Vec::new();
        let words: Vec<&str> = text.split_whitespace().collect();

        for window in words.windows(2) {
            let phrase = window.join(" ");
            // Look for "Street", "Avenue", "City" patterns
            if phrase.contains("Street")
                || phrase.contains("Ave")
                || phrase.contains("Boulevard")
                || phrase.contains("Road")
            {
                locations.push(phrase);
            }
        }

        locations
    }

    fn extract_temporal_refs(&self, text: &str) -> Vec<String> {
        let mut refs = Vec::new();

        // Time patterns
        let time_regex = regex::Regex::new(r"\d{1,2}:\d{2}(?:\s*(?:am|pm))?").unwrap();
        for cap in time_regex.find_iter(text) {
            refs.push(cap.as_str().to_string());
        }

        // Date patterns
        let date_regex = regex::Regex::new(r"\d{1,2}/\d{1,2}/\d{2,4}").unwrap();
        for cap in date_regex.find_iter(text) {
            refs.push(cap.as_str().to_string());
        }

        refs
    }

    fn build_thematic_analysis(&self, forumyze: &ForumyzeResult) -> ThematicAnalysis {
        let themes: Vec<Theme> = forumyze
            .discussions
            .iter()
            .map(|d| {
                Theme {
                    id: d.id.clone(),
                    name: d.title.clone(),
                    description: d.description.clone(),
                    comment_count: d.total_comments,
                    representative_quotes: vec![], // Would extract from comments
                    sentiment: 0.0,
                }
            })
            .collect();

        // Fallback to topics if no discussions
        let themes = if themes.is_empty() {
            forumyze
                .topics
                .iter()
                .enumerate()
                .map(|(i, topic)| Theme {
                    id: format!("topic_{}", i),
                    name: topic.clone(),
                    description: String::new(),
                    comment_count: 0,
                    representative_quotes: vec![],
                    sentiment: 0.0,
                })
                .collect()
        } else {
            themes
        };

        ThematicAnalysis {
            themes,
            sentiment_breakdown: SentimentBreakdown {
                positive: 0.2,
                negative: 0.5,
                neutral: 0.3,
                outrage: 0.4,
                sympathy: 0.3,
                calls_for_justice: 0.35,
            },
            narratives: vec![],
            disputed_facts: vec![],
        }
    }

    fn extract_timeline(&self, statements: &[WitnessStatement]) -> Vec<TimelineEvent> {
        let mut events = Vec::new();

        for stmt in statements {
            if !stmt.temporal_references.is_empty() {
                events.push(TimelineEvent {
                    timestamp: stmt.temporal_references[0].clone(),
                    description: stmt.statement.chars().take(200).collect(),
                    source_type: TimelineSourceType::WitnessComment,
                    source_ids: vec![stmt.comment_id.clone()],
                    confidence: stmt.relevance_score,
                });
            }
        }

        events
    }

    fn count_unique_authors(&self, forumyze: &ForumyzeResult) -> usize {
        let mut authors = HashSet::new();
        for comment in &forumyze.core_dialogue {
            authors.insert(&comment.author);
        }
        for comment in &forumyze.questions {
            authors.insert(&comment.author);
        }
        for comment in &forumyze.anomalies {
            authors.insert(&comment.author);
        }
        authors.len()
    }

    fn get_date_range(&self, statements: &[WitnessStatement]) -> Option<(String, String)> {
        if statements.is_empty() {
            return None;
        }

        let timestamps: Vec<&str> = statements
            .iter()
            .map(|s| s.timestamp.as_str())
            .filter(|t| !t.is_empty())
            .collect();

        if timestamps.is_empty() {
            return None;
        }

        Some((
            timestamps.iter().min().unwrap().to_string(),
            timestamps.iter().max().unwrap().to_string(),
        ))
    }

    fn calc_avg_engagement(&self, forumyze: &ForumyzeResult) -> f32 {
        let total_likes: u64 = forumyze.core_dialogue.iter().map(|c| c.like_count).sum();
        let count = forumyze.core_dialogue.len();
        if count == 0 {
            0.0
        } else {
            total_likes as f32 / count as f32
        }
    }

    fn calculate_package_hash(&self, id: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        id.hash(&mut hasher);
        Utc::now().timestamp().hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

// ============================================================================
// EXPORT FORMATS
// ============================================================================

impl EvidencePackage {
    /// Export to JSON
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| format!("JSON serialization failed: {}", e))
    }

    /// Export to markdown report
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str(&format!("# Evidence Package: {}\n\n", self.id));

        if let Some(ref case) = self.case_reference {
            md.push_str(&format!("**Case Reference:** {}\n\n", case));
        }

        md.push_str(&format!(
            "**Generated:** {}\n\n",
            self.metadata.generated_at
        ));

        md.push_str("---\n\n## Video Evidence\n\n");
        md.push_str(&format!("- **Title:** {}\n", self.video_evidence.title));
        md.push_str(&format!("- **Channel:** {}\n", self.video_evidence.channel));
        md.push_str(&format!("- **URL:** {}\n", self.video_evidence.url));
        md.push_str(&format!(
            "- **Views:** {}\n",
            self.video_evidence.view_count
        ));
        md.push_str(&format!(
            "- **Comments:** {}\n\n",
            self.video_evidence.comment_count
        ));

        md.push_str("---\n\n## Statistics\n\n");
        md.push_str(&format!(
            "- Total comments analyzed: {}\n",
            self.statistics.total_comments_analyzed
        ));
        md.push_str(&format!(
            "- Witness statements extracted: {}\n",
            self.statistics.witness_statements_extracted
        ));
        md.push_str(&format!(
            "- First-hand accounts: {}\n",
            self.statistics.first_hand_accounts
        ));
        md.push_str(&format!(
            "- Unique authors: {}\n",
            self.statistics.unique_authors
        ));
        md.push_str(&format!(
            "- Related case law: {}\n\n",
            self.statistics.case_law_references
        ));

        md.push_str("---\n\n## Key Witness Statements\n\n");
        for (i, stmt) in self.witness_statements.iter().take(20).enumerate() {
            md.push_str(&format!(
                "### {}. {} ({:?})\n\n",
                i + 1,
                stmt.author,
                stmt.classification
            ));
            md.push_str(&format!("> {}\n\n", stmt.statement));
            md.push_str(&format!(
                "*Likes: {} | First-hand: {} | Relevance: {:.2}*\n\n",
                stmt.like_count, stmt.first_hand, stmt.relevance_score
            ));
        }

        if !self.case_law.is_empty() {
            md.push_str("---\n\n## Related Case Law\n\n");
            for case in &self.case_law {
                md.push_str(&format!("### {}\n\n", case.case_name));
                md.push_str(&format!("- **Citation:** {}\n", case.citation));
                md.push_str(&format!("- **Court:** {}\n", case.court));
                md.push_str(&format!("- **Date:** {}\n", case.date_filed));
                md.push_str(&format!("- **Times Cited:** {}\n", case.cite_count));
                md.push_str(&format!("- [View on CourtListener]({})\n\n", case.url));
            }
        }

        md
    }

    /// Save package to file
    pub async fn save(&self, path: &PathBuf) -> Result<(), String> {
        let json = self.to_json()?;
        tokio::fs::write(path, json)
            .await
            .map_err(|e| format!("Failed to save package: {}", e))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_statement() {
        let builder = EvidencePackageBuilder::new(None);

        assert!(matches!(
            builder.classify_statement("I saw the whole thing happen from my window"),
            StatementClassification::Eyewitness
        ));

        assert!(matches!(
            builder.classify_statement("As a former police officer, this is wrong"),
            StatementClassification::Expert
        ));

        assert!(matches!(
            builder.classify_statement("What time did this happen?"),
            StatementClassification::Question
        ));
    }

    #[test]
    fn test_is_first_hand() {
        let builder = EvidencePackageBuilder::new(None);

        assert!(builder.is_first_hand("I was there when it happened"));
        assert!(builder.is_first_hand("I live in that neighborhood"));
        assert!(!builder.is_first_hand("This is terrible, prayers for the family"));
    }
}
