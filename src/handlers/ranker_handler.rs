use axum::extract::{Path, State, Query};
use axum::http::header;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

use crate::app_state::AppState;
use crate::errors::AppError;
use crate::models::ranker::{RankerRequest, RankerResponse};
use crate::services::ranker;

// fetch contest title from vjudge
pub async fn get_contest_title(
    Path(id): Path<u64>,
) -> Result<Json<Value>, AppError> {
    let contest = crate::services::vjudge::fetch_contest(id).await?;
    Ok(Json(json!({
        "success": true,
        "title": contest.title
    })))
}

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
    let result = ranker::analyze(&state.pool, &body).await?;

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

#[derive(Debug, serde::Deserialize)]
pub struct PdfQuery {
    pub include_details: Option<bool>,
}

// generate and download a branded pdf of the rankings
pub async fn download_pdf(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<PdfQuery>,
) -> Result<impl IntoResponse, AppError> {
    // look up cached result
    let result = {
        let cache = state.results_cache.lock().unwrap();
        cache.get(&session_id).cloned()
    };

    let result = result.ok_or(AppError::NotFound(
        "Session not found — please run /analyze first".to_string(),
    ))?;

    let include_details = query.include_details.unwrap_or(true);
    let pdf_bytes = generate_pdf(&result, include_details)?;

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
fn generate_pdf(result: &RankerResponse, include_details: bool) -> Result<Vec<u8>, AppError> {
    use genpdf::elements::{Paragraph, PaddedElement, TableLayout};
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
            .styled(style::Style::new().bold().with_font_size(22)),
    );

    doc.push(
        Paragraph::new("VJudge Standing")
            .aligned(Alignment::Center)
            .styled(style::Style::new().bold().with_font_size(16)),
    );

    doc.push(Paragraph::new(""));

    // --- title ---

    doc.push(
        Paragraph::new(&result.title)
            .aligned(Alignment::Center)
            .styled(style::Style::new().bold().with_font_size(13)),
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
            .styled(style::Style::new().with_font_size(10)),
    );

    doc.push(
        Paragraph::new(format!(
            "Total Contests: {}  |  Total Participants: {}",
            result.total_contests, result.total_participants,
        ))
        .aligned(Alignment::Center)
        .styled(style::Style::new().with_font_size(10)),
    );

    doc.push(Paragraph::new(""));

    // --- rankings table ---
    // columns: Rank | Handle | Contest Count | Solved | Penalty | Upsolved | Total Solved

    // helper macro for padded cells
    macro_rules! pad {
        ($elem:expr) => {
            PaddedElement::new($elem, 1)
        };
    }

    let header_style = style::Style::new().bold().with_font_size(10);
    let row_style = style::Style::new().with_font_size(10);
    let detail_style = style::Style::new().italic().with_font_size(8);

    // column widths proportional: wider columns for text headers
    let col_widths = vec![1, 3, 2, 2, 2, 2, 2];

    // manually paginate to fix genpdf's broken borders at page boundaries
    let mut is_first_page = true;
    let mut current_idx = 0;

    while current_idx < result.rankings.len() {
        if !is_first_page {
            doc.push(genpdf::elements::PageBreak::new());
        }

        let mut table = TableLayout::new(col_widths.clone());
        table.set_cell_decorator(genpdf::elements::FrameCellDecorator::new(true, true, false));

        // table header row
        let mut header_row = table.row();
        header_row.push_element(pad!(Paragraph::new("Rank").styled(header_style.clone())));
        header_row.push_element(pad!(Paragraph::new("Handle").styled(header_style.clone())));
        header_row.push_element(pad!(Paragraph::new("Contests Count").styled(header_style.clone())));
        header_row.push_element(pad!(Paragraph::new("Solved").styled(header_style.clone())));
        header_row.push_element(pad!(Paragraph::new("Penalty").styled(header_style.clone())));
        header_row.push_element(pad!(Paragraph::new("Upsolved").styled(header_style.clone())));
        header_row.push_element(pad!(Paragraph::new("Total Solved").styled(header_style.clone())));
        header_row.push().ok();

        // 25 rows fit on the first page (with title/header), 36 on subsequent pages
        let max_rows = if is_first_page { 25 } else { 36 };
        let mut rendered_rows = 1; // header counts as 1
        let mut end_idx = current_idx;

        while end_idx < result.rankings.len() {
            let participant_rows = if include_details {
                1 + result.rankings[end_idx].contest_details.len()
            } else {
                1
            };
            if rendered_rows + participant_rows > max_rows {
                if rendered_rows > 1 {
                    break;
                }
            }
            rendered_rows += participant_rows;
            end_idx += 1;
        }

        for p in &result.rankings[current_idx..end_idx] {
            let total_solved = p.problems_solved + p.total_upsolved;

            let mut row = table.row();
            row.push_element(pad!(Paragraph::new(p.rank.to_string()).styled(row_style.clone())));
            // for merged handles (comma-separated), render each on its own line
            if p.handle.contains(',') {
                let handles: Vec<&str> = p.handle.split(',').collect();
                let mut layout = genpdf::elements::LinearLayout::vertical();
                for (i, h) in handles.iter().enumerate() {
                    let text = if i < handles.len() - 1 {
                        format!("{} ,", h.trim())
                    } else {
                        h.trim().to_string()
                    };
                    layout.push(Paragraph::new(text).styled(row_style.clone()));
                }
                row.push_element(pad!(layout));
            } else {
                row.push_element(pad!(Paragraph::new(p.handle.clone()).styled(row_style.clone())));
            }
            row.push_element(pad!(Paragraph::new(p.contests_participated.to_string()).styled(row_style.clone())));
            row.push_element(pad!(Paragraph::new(p.problems_solved.to_string()).styled(row_style.clone())));
            row.push_element(pad!(Paragraph::new(p.total_penalty.to_string()).styled(row_style.clone())));
            row.push_element(pad!(Paragraph::new(p.total_upsolved.to_string()).styled(row_style.clone())));
            row.push_element(pad!(Paragraph::new(total_solved.to_string()).styled(row_style.clone())));
            row.push().ok();

            if include_details {
                for detail in &p.contest_details {
                    let detail_total = detail.solved + detail.upsolved;
                    let mut detail_row = table.row();
                    detail_row.push_element(pad!(Paragraph::new("").styled(detail_style.clone())));
                    detail_row.push_element(pad!(Paragraph::new(format!("  └─ {}", detail.contest_name)).styled(detail_style.clone())));
                    let part_val = if detail.participated { "1" } else { "0" };
                    detail_row.push_element(pad!(Paragraph::new(part_val).styled(detail_style.clone())));
                    detail_row.push_element(pad!(Paragraph::new(detail.solved.to_string()).styled(detail_style.clone())));
                    detail_row.push_element(pad!(Paragraph::new(detail.penalty.to_string()).styled(detail_style.clone())));
                    detail_row.push_element(pad!(Paragraph::new(detail.upsolved.to_string()).styled(detail_style.clone())));
                    detail_row.push_element(pad!(Paragraph::new(detail_total.to_string()).styled(detail_style.clone())));
                    detail_row.push().ok();
                }
            }
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

//    doc.push(
//        Paragraph::new("Powered by SUST CP Geeks Platform")
//            .aligned(Alignment::Center)
//            .styled(style::Style::new().italic().with_font_size(9)),
//    );

    // render to bytes
    let mut buf = Vec::new();
    doc.render(&mut buf)
        .map_err(|e| AppError::InternalError(format!("Failed to render PDF: {}", e)))?;

    Ok(buf)
}
