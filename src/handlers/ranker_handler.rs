use axum::extract::{Path, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

use crate::app_state::AppState;
use crate::errors::AppError;
use crate::models::ranker::{RankerRequest, RankerResponse};
use crate::services::ranker;

// analyze contests and return ranked results
pub async fn analyze(
    State(state): State<AppState>,
    Json(body): Json<RankerRequest>,
) -> Result<Json<Value>, AppError> {
    // validate input
    if body.title.trim().is_empty() {
        return Err(AppError::BadRequest("Title is required".to_string()));
    }
    if body.contest_ids.is_empty() {
        return Err(AppError::BadRequest(
            "At least one contest ID is required".to_string(),
        ));
    }

    // run the ranking algorithm
    let result = ranker::analyze(&body).await?;

    // cache the result for pdf download
    let session_id = uuid::Uuid::new_v4().to_string();
    {
        let mut cache = state.results_cache.lock().unwrap();
        cache.insert(session_id.clone(), result.clone());
    }

    Ok(Json(json!({
        "success": true,
        "session_id": session_id,
        "data": result
    })))
}

// generate and download a branded pdf of the rankings
pub async fn download_pdf(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    // look up cached result
    let result = {
        let cache = state.results_cache.lock().unwrap();
        cache.get(&session_id).cloned()
    };

    let result = result.ok_or(AppError::NotFound(
        "Session not found — please run /analyze first".to_string(),
    ))?;

    let pdf_bytes = generate_pdf(&result)?;

    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"rankings.pdf\"",
            ),
        ],
        pdf_bytes,
    ))
}

// builds a branded pdf document from the ranking results
fn generate_pdf(result: &RankerResponse) -> Result<Vec<u8>, AppError> {
    use genpdf::elements::{Paragraph, TableLayout};
    use genpdf::{style, Alignment, Element as _};

    let font_family = genpdf::fonts::from_files("./fonts", "LiberationSans", None)
        .map_err(|e| AppError::InternalError(format!("Failed to load fonts: {}", e)))?;

    let mut doc = genpdf::Document::new(font_family);
    doc.set_title(&result.title);

    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(20);
    doc.set_page_decorator(decorator);

    // --- header section (centered) ---

    doc.push(
        Paragraph::new("SUST CP Geeks")
            .aligned(Alignment::Center)
            .styled(style::Style::new().bold().with_font_size(24)),
    );

    doc.push(
        Paragraph::new("VJudge Standing")
            .aligned(Alignment::Center)
            .styled(style::Style::new().bold().with_font_size(18)),
    );

    doc.push(Paragraph::new(""));

    // --- title ---

    doc.push(
        Paragraph::new(&result.title)
            .aligned(Alignment::Center)
            .styled(style::Style::new().bold().with_font_size(14)),
    );

    doc.push(Paragraph::new(""));

    // --- contest info ---

    let ids_str = result
        .contest_ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    doc.push(
        Paragraph::new(format!("Contest IDs: {}", ids_str))
            .aligned(Alignment::Center)
            .styled(style::Style::new().with_font_size(11)),
    );

    doc.push(
        Paragraph::new(format!(
            "Total Contests: {}  |  Total Participants: {}",
            result.total_contests, result.total_participants,
        ))
        .aligned(Alignment::Center)
        .styled(style::Style::new().with_font_size(11)),
    );

    doc.push(Paragraph::new(""));
    doc.push(Paragraph::new(""));

    // --- rankings table ---
    use genpdf::elements::PaddedElement;

    let mut table = TableLayout::new(vec![1, 4, 2, 2, 2]);
    table.set_cell_decorator(genpdf::elements::FrameCellDecorator::new(true, true, false));

    // helper macro for padded cells
    macro_rules! pad {
        ($elem:expr) => {
            PaddedElement::new($elem, 1)
        };
    }

    let header_style = style::Style::new().bold().with_font_size(12);
    let row_style = style::Style::new().with_font_size(11);

    // we manually paginate to fix genpdf's broken borders at page boundaries
    // and to repeat the header row on every page.
    let mut is_first_page = true;
    let mut current_idx = 0;

    while current_idx < result.rankings.len() {
        if !is_first_page {
            doc.push(genpdf::elements::PageBreak::new());
        }

        let mut table = TableLayout::new(vec![1, 4, 2, 2, 2]);
        table.set_cell_decorator(genpdf::elements::FrameCellDecorator::new(true, true, false));

        // table header row
        let mut header_row = table.row();
        header_row.push_element(pad!(Paragraph::new("Rank").styled(header_style.clone())));
        header_row.push_element(pad!(Paragraph::new("Handle").styled(header_style.clone())));
        header_row.push_element(pad!(Paragraph::new("Score").styled(header_style.clone())));
        header_row.push_element(pad!(Paragraph::new("Solved").styled(header_style.clone())));
        header_row.push_element(pad!(Paragraph::new("Penalty").styled(header_style.clone())));
        header_row.push().ok();

        // 25 rows fit comfortably on the first page (with title), 36 rows fit on subsequent pages
        let chunk_size = if is_first_page { 25 } else { 36 };
        let end_idx = (current_idx + chunk_size).min(result.rankings.len());

        for p in &result.rankings[current_idx..end_idx] {
            let mut row = table.row();
            row.push_element(pad!(Paragraph::new(p.rank.to_string()).styled(row_style.clone())));
            row.push_element(pad!(Paragraph::new(&p.handle).styled(row_style.clone())));
            row.push_element(pad!(Paragraph::new(format!("{:.0}", p.total_score)).styled(row_style.clone())));
            row.push_element(pad!(Paragraph::new(p.problems_solved.to_string()).styled(row_style.clone())));
            row.push_element(pad!(Paragraph::new(p.total_penalty.to_string()).styled(row_style.clone())));
            row.push().ok();
        }

        doc.push(table);
        current_idx = end_idx;
        is_first_page = false;
    }
    doc.push(Paragraph::new(""));
    doc.push(Paragraph::new(""));

    // --- footer ---

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    doc.push(
        Paragraph::new(format!("Generated on {}", now))
            .aligned(Alignment::Center)
            .styled(style::Style::new().italic().with_font_size(9)),
    );

    doc.push(
        Paragraph::new("Powered by SUST CP Geeks Platform")
            .aligned(Alignment::Center)
            .styled(style::Style::new().italic().with_font_size(9)),
    );

    // render to bytes
    let mut buf = Vec::new();
    doc.render(&mut buf)
        .map_err(|e| AppError::InternalError(format!("Failed to render PDF: {}", e)))?;

    Ok(buf)
}
