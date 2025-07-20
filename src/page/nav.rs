use crate::node;

lazy_static::lazy_static! {
    pub static ref NAVBAR: String = render();
}

pub fn render() -> String {
    node! { nav, class = "navbar" =>
        node! { div, class = "nav-container" =>
            node!{ a, class="nav-button", href = "/music" => "Music Recs" },
            node!{ a, class="nav-button", href = "/words" => "Words" },
        },
    }
    .to_string()
}
