// Cursor pagination dùng chung cho mọi endpoint trả về danh sách. Cursor là 1 chuỗi
// base64 mã hoá JSON của (các) cột sort cuối cùng đã thấy — dùng keyset pagination
// (WHERE (sort_cols) < cursor) thay vì OFFSET để ổn định khi dữ liệu vẫn đang được ghi thêm.
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Serialize, de::DeserializeOwned};

pub const DEFAULT_LIMIT: i64 = 50;
pub const MAX_LIMIT: i64 = 200;

pub fn clamp_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

pub fn encode_cursor<T: Serialize>(value: &T) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).unwrap_or_default())
}

/// Cursor không hợp lệ (hỏng/giả mạo) được coi như "không có cursor" — trả về từ trang đầu
/// thay vì lỗi 400, vì đây chỉ là gợi ý vị trí tiếp tục, không phải input nghiệp vụ.
pub fn decode_cursor<T: DeserializeOwned>(cursor: Option<&str>) -> Option<T> {
    let cursor = cursor?;
    let bytes = URL_SAFE_NO_PAD.decode(cursor).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[derive(Serialize)]
pub struct Page<T: Serialize> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

/// `rows` phải được fetch với LIMIT = limit + 1 để hàm này biết còn trang sau hay không.
pub fn paginate<T: Serialize, K: Serialize>(
    mut rows: Vec<T>,
    limit: i64,
    cursor_key: impl Fn(&T) -> K,
) -> Page<T> {
    let has_more = rows.len() as i64 > limit;
    if has_more {
        rows.truncate(limit as usize);
    }
    let next_cursor = has_more
        .then(|| rows.last().map(|last| encode_cursor(&cursor_key(last))))
        .flatten();
    Page {
        items: rows,
        next_cursor,
    }
}
