# frozen_string_literal: true

module Selma
  class Sanitizer
    module Config
      RELAXED = freeze_config(
        elements: BASIC[:elements] + ["address", "article", "aside", "bdi", "bdo", "body", "caption", "col",
                                      "colgroup", "data", "del", "div", "figcaption", "figure", "footer", "h1", "h2", "h3", "h4", "h5", "h6", "head", "header", "hgroup", "hr", "html", "img", "ins", "main", "nav", "rp", "rt", "ruby", "section", "span", "style", "summary", "sup", "table", "tbody", "td", "tfoot", "th", "thead", "title", "tr", "wbr",],

        allow_doctype: true,

        attributes: merge(BASIC[:attributes],
          :all => ["class", "dir", "hidden", "id", "lang", "style", "tabindex", "title", "translate"],
          "a" => ["href", "hreflang", "name", "rel"],
          "col" => ["span", "width"],
          "colgroup" => ["span", "width"],
          "data" => ["value"],
          "del" => ["cite", "datetime"],
          "img" => ["align", "alt", "border", "height", "src", "srcset", "width"],
          "ins" => ["cite", "datetime"],
          "li" => ["value"],
          "ol" => ["reversed", "start", "type"],
          "style" => ["media", "scoped", "type"],
          "table" => ["align", "bgcolor", "border", "cellpadding", "cellspacing", "frame", "rules", "sortable",
                      "summary", "width",],
          "td" => ["abbr", "align", "axis", "colspan", "headers", "rowspan", "valign", "width"],
          "th" => ["abbr", "align", "axis", "colspan", "headers", "rowspan", "scope", "sorted", "valign", "width"],
          "ul" => ["type"]),

        protocols: merge(BASIC[:protocols],
          "del" => { "cite" => ["http", "https", :relative] },
          "img" => { "src"  => ["http", "https", :relative] },
          "ins" => { "cite" => ["http", "https", :relative] })
      )
    end
  end
end
