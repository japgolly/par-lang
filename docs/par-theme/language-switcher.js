// Adds a language menu to the menu bar, linking to the same page in each language's book.
//
// The books are deployed as sibling directories (`/book/`, `/book-ko/`, ...), so the links
// are relative: they swap the book directory and keep the path within the book. If a page
// doesn't exist in another book, its link falls back to that book's index.
//
// To add a language, add it to `books` below and set `language` in its `book.toml`.
//
// This file is shared verbatim between the `par-theme` directories of all books.

(function () {
    const books = [
        { lang: "en", dir: "book", label: "English" },
        { lang: "ko", dir: "book-ko", label: "한국어" },
    ];
    const current = document.documentElement.lang;

    // `path_to_root` is empty for pages at the root of the book, which would resolve to the
    // page itself. `baseURI`, not `location`: the 404 page sets `<base href="/">`.
    const root = new URL(path_to_root || "./", document.baseURI);
    const page = new URL(location.href);
    page.search = "";
    page.hash = "";
    const pathInBook = page.href.startsWith(root.href) ? page.href.slice(root.href.length) : "";

    const buttons = document.querySelector("#mdbook-menu-bar .right-buttons");
    if (!buttons) return;

    const wrapper = document.createElement("div");
    wrapper.className = "language-switcher";

    // Font Awesome's "language" icon, the same icon set mdBook uses for its own buttons.
    const toggle = document.createElement("button");
    toggle.className = "icon-button";
    toggle.type = "button";
    toggle.title = "Language";
    toggle.setAttribute("aria-label", "Change language");
    toggle.setAttribute("aria-haspopup", "true");
    toggle.setAttribute("aria-expanded", "false");
    toggle.setAttribute("aria-controls", "language-list");
    toggle.innerHTML = '<span class="fa-svg"><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 512"><!--! Font Awesome Free 6.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free (Icons: CC BY 4.0, Fonts: SIL OFL 1.1, Code: MIT License) Copyright 2022 Fonticons, Inc. --><path d="M0 128C0 92.7 28.7 64 64 64H256h48 16H576c35.3 0 64 28.7 64 64V384c0 35.3-28.7 64-64 64H320 304 256 64c-35.3 0-64-28.7-64-64V128zm320 0V384H576V128H320zM178.3 175.9c-3.2-7.2-10.4-11.9-18.3-11.9s-15.1 4.7-18.3 11.9l-64 144c-4.5 10.1 .1 21.9 10.2 26.4s21.9-.1 26.4-10.2l8.9-20.1h73.6l8.9 20.1c4.5 10.1 16.3 14.6 26.4 10.2s14.6-16.3 10.2-26.4l-64-144zM160 233.2L179 276H141l19-42.8zM448 164c11 0 20 9 20 20v4h44 16c11 0 20 9 20 20s-9 20-20 20h-2l-1.6 4.5c-8.9 24.4-22.4 46.6-39.6 65.4c.9 .6 1.8 1.1 2.7 1.6l18.9 11.3c9.5 5.7 12.5 18 6.9 27.4s-18 12.5-27.4 6.9l-18.9-11.3c-4.5-2.7-8.8-5.5-13.1-8.5c-10.6 7.5-21.9 14-34 19.4l-3.6 1.6c-10.1 4.5-21.9-.1-26.4-10.2s.1-21.9 10.2-26.4l3.6-1.6c6.4-2.9 12.6-6.1 18.5-9.8l-12.2-12.2c-7.8-7.8-7.8-20.5 0-28.3s20.5-7.8 28.3 0l14.6 14.6 .5 .5c12.4-13.1 22.5-28.3 29.8-45H448 376c-11 0-20-9-20-20s9-20 20-20h52v-4c0-11 9-20 20-20z"/></svg></span>';

    // Styled like mdBook's theme menu. Its script only queries inside its own popup.
    const list = document.createElement("ul");
    list.id = "language-list";
    list.className = "theme-popup";
    list.setAttribute("role", "menu");
    list.setAttribute("aria-label", "Languages");

    const checks = [];
    for (const book of books) {
        const index = new URL(`../${book.dir}/`, root);
        const target = new URL(pathInBook, index);

        const link = document.createElement("a");
        link.className = "theme";
        link.setAttribute("role", "menuitem");
        link.href = target.href;
        link.lang = book.lang;
        link.hreflang = book.lang;
        link.textContent = book.label;

        if (book.lang === current) {
            link.classList.add("theme-selected");
            link.setAttribute("aria-current", "page");
        } else if (target.href !== index.href) {
            checks.push(() =>
                fetch(target, { method: "HEAD" })
                    .then((response) => {
                        if (response.status === 404) link.href = index.href;
                    })
                    .catch(() => {})
            );
        }

        const item = document.createElement("li");
        item.setAttribute("role", "none");
        item.append(link);
        list.append(item);
    }

    // Check which pages exist only once the reader shows interest in the menu.
    let checked = false;
    function checkPages() {
        if (checked) return;
        checked = true;
        checks.forEach((check) => check());
    }

    function isOpen() {
        return list.style.display === "block";
    }

    function setOpen(open) {
        list.style.display = open ? "block" : "none";
        toggle.setAttribute("aria-expanded", String(open));
        if (open) checkPages();
    }

    toggle.addEventListener("pointerenter", checkPages);
    toggle.addEventListener("focus", checkPages);
    toggle.addEventListener("click", () => setOpen(!isOpen()));
    document.addEventListener("click", (event) => {
        if (!wrapper.contains(event.target)) setOpen(false);
    });
    wrapper.addEventListener("keydown", (event) => {
        if (event.key === "Escape" && isOpen()) {
            event.stopPropagation();
            setOpen(false);
            toggle.focus();
        }
    });

    wrapper.append(toggle, list);
    buttons.prepend(wrapper);
})();
