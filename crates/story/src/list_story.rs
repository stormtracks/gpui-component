use core::time;

use fake::Fake;
use gpui::{
    actions, div, px, relative, AnyElement, ElementId, FocusHandle, FocusableView,
    InteractiveElement, IntoElement, ParentElement, Render, RenderOnce, Styled, Task, Timer, View,
    ViewContext, VisualContext, WindowContext,
};

use ui::{
    h_flex,
    label::Label,
    list::{List, ListDelegate, ListItem},
    theme::{hsl, ActiveTheme},
    v_flex,
};

actions!(list_story, [SelectedCompany]);

#[derive(Clone)]
struct Company {
    name: String,
    industry: String,
    last_done: f64,
    prev_close: f64,
    // description: String,
}

impl Company {
    fn random_update(&mut self) {
        self.last_done = self.prev_close * (1.0 + (-0.2..0.2).fake::<f64>());
    }

    fn change_percent(&self) -> f64 {
        (self.last_done - self.prev_close) / self.prev_close
    }
}

#[derive(IntoElement)]
struct CompanyListItem {
    base: ListItem,
    ix: usize,
    company: Company,
    selected: bool,
}

impl CompanyListItem {
    pub fn new(id: impl Into<ElementId>, company: Company, ix: usize, selected: bool) -> Self {
        CompanyListItem {
            company,
            ix,
            base: ListItem::new(id),
            selected,
        }
    }
}

impl RenderOnce for CompanyListItem {
    fn render(self, cx: &mut WindowContext) -> impl IntoElement {
        let text_color = if self.selected {
            cx.theme().accent_foreground
        } else {
            cx.theme().foreground
        };

        let trend_color = match self.company.change_percent() {
            change if change > 0.0 => hsl(0.0, 79.0, 53.0),
            change if change < 0.0 => hsl(100.0, 79.0, 53.0),
            _ => cx.theme().foreground,
        };

        let bg_color = if self.selected {
            cx.theme().list_active
        } else if self.ix % 2 == 0 {
            cx.theme().list
        } else {
            cx.theme().list_even
        };

        self.base
            .px_3()
            .py_1()
            .overflow_x_hidden()
            .bg(bg_color)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .text_color(text_color)
                    .child(
                        v_flex()
                            .gap_1()
                            .max_w(px(500.))
                            .overflow_x_hidden()
                            .flex_nowrap()
                            .child(Label::new(self.company.name.clone()).whitespace_nowrap())
                            .child(
                                div().text_sm().overflow_x_hidden().child(
                                    Label::new(self.company.industry.clone())
                                        .whitespace_nowrap()
                                        .text_color(text_color.opacity(0.5)),
                                ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .justify_end()
                            .child(
                                div()
                                    .w(px(65.))
                                    .text_color(text_color)
                                    .child(format!("{:.2}", self.company.last_done)),
                            )
                            .child(
                                h_flex().w(px(65.)).justify_end().child(
                                    div()
                                        .rounded_md()
                                        .whitespace_nowrap()
                                        .text_size(px(12.))
                                        .px_1()
                                        .text_color(trend_color)
                                        .child(format!("{:.2}%", self.company.change_percent())),
                                ),
                            ),
                    ),
            )
    }
}

struct CompanyListDelegate {
    companies: Vec<Company>,
    matched_companies: Vec<Company>,
    selected_index: usize,
    confirmed_index: Option<usize>,
}

impl ListDelegate for CompanyListDelegate {
    type Item = CompanyListItem;

    fn items_count(&self) -> usize {
        self.matched_companies.len()
    }

    fn confirmed_index(&self) -> Option<usize> {
        self.confirmed_index
    }

    fn perform_search(&mut self, query: &str, _: &mut ViewContext<List<Self>>) -> Task<()> {
        self.matched_companies = self
            .companies
            .iter()
            .filter(|company| company.name.to_lowercase().contains(&query.to_lowercase()))
            .cloned()
            .collect();

        Task::Ready(Some(()))
    }

    fn confirm(&mut self, ix: Option<usize>, cx: &mut ViewContext<List<Self>>) {
        self.confirmed_index = ix;
        if let Some(_) = ix {
            cx.dispatch_action(Box::new(SelectedCompany));
        }
    }

    fn render_initial(&self, cx: &mut ViewContext<List<Self>>) -> Option<AnyElement> {
        let histories = ["BABA", "BIDU", "GOOGL", "LB", "LP", "LBW"];

        let input_history = histories
            .into_iter()
            .map(|name| {
                div()
                    .rounded_xl()
                    .min_w(px(30.))
                    .border_1()
                    .rounded_md()
                    .border_color(cx.theme().muted_foreground.opacity(0.3))
                    .line_height(relative(1.))
                    .p_1()
                    .child(div().child(name).text_xs())
            })
            .collect::<Vec<_>>();

        let element = v_flex()
            .p_4()
            .child(
                v_flex()
                    .gap_y_2()
                    .child("History")
                    .child(h_flex().gap_x_4().children(input_history)),
            )
            .into_any_element();
        Some(element)
    }

    fn set_selected_index(&mut self, ix: Option<usize>, cx: &mut ViewContext<List<Self>>) {
        if let Some(ix) = ix {
            self.selected_index = ix;
            cx.notify();
        }
    }

    fn render_item(&self, ix: usize, _cx: &mut ViewContext<List<Self>>) -> Option<Self::Item> {
        let selected = ix == self.selected_index || Some(ix) == self.confirmed_index;
        if let Some(company) = self.matched_companies.get(ix) {
            return Some(CompanyListItem::new(ix, company.clone(), ix, selected));
        }

        None
    }
}

impl CompanyListDelegate {
    fn selected_company(&self) -> Option<Company> {
        self.companies.get(self.selected_index).cloned()
    }
}

pub struct ListStory {
    focus_handle: FocusHandle,
    company_list: View<List<CompanyListDelegate>>,
    selected_company: Option<Company>,
}

impl super::Story for ListStory {
    fn title() -> &'static str {
        "List"
    }

    fn description() -> &'static str {
        "A list displays a series of items."
    }

    fn new_view(cx: &mut WindowContext) -> View<impl gpui::FocusableView> {
        Self::view(cx)
    }
}

impl ListStory {
    pub fn view(cx: &mut WindowContext) -> View<Self> {
        cx.new_view(Self::new)
    }

    fn new(cx: &mut ViewContext<Self>) -> Self {
        let companies = (0..1_000)
            .map(|_| random_company())
            .collect::<Vec<Company>>();

        let company_list = cx.new_view(|cx| {
            List::new(
                CompanyListDelegate {
                    matched_companies: companies.clone(),
                    companies,
                    selected_index: 0,
                    confirmed_index: None,
                },
                cx,
            )
        });

        // Spawn a background to random refresh the list
        cx.spawn(move |this, mut cx| async move {
            loop {
                Timer::after(time::Duration::from_secs_f64(0.5)).await;
                this.update(&mut cx, |this, cx| {
                    this.company_list.update(cx, |picker, _| {
                        picker
                            .delegate_mut()
                            .companies
                            .iter_mut()
                            .for_each(|company| {
                                company.random_update();
                            });
                    });
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            company_list,
            selected_company: None,
        }
    }

    fn selected_company(&mut self, _: &SelectedCompany, cx: &mut ViewContext<Self>) {
        let picker = self.company_list.read(cx);
        if let Some(company) = picker.delegate().selected_company() {
            self.selected_company = Some(company);
        }
    }
}

fn random_company() -> Company {
    let last_done = (0.0..999.0).fake::<f64>();
    let prev_close = last_done * (-0.1..0.1).fake::<f64>();
    Company {
        name: fake::faker::company::en::CompanyName().fake(),
        industry: fake::faker::company::en::Industry().fake(),
        last_done,
        prev_close,
    }
}

impl FocusableView for ListStory {
    fn focus_handle(&self, _cx: &gpui::AppContext) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ListStory {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::selected_company))
            .size_full()
            .gap_4()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(self.company_list.clone())
    }
}
