(function () {
    var langs = [
        { code: "ru", label: "🇷🇺 Русский" },
        { code: "en", label: "🇬🇧 English" },
        { code: "pt-br", label: "🇧🇷 Português" },
    ];

    var currentPath = window.location.pathname;

    function detectLang() {
        for (var i = 0; i < langs.length; i++) {
            if (currentPath.indexOf("/" + langs[i].code + "/") !== -1) {
                return langs[i].code;
            }
        }
        return "en";
    }

    function switchUrl(targetLang) {
        var current = detectLang();
        if (current === targetLang) return currentPath;
        return currentPath.replace("/" + current + "/", "/" + targetLang + "/");
    }

    function createSwitcher() {
        var currentLang = detectLang();

        var container = document.createElement("div");
        container.className = "lang-switcher";

        var currentItem = langs.find(function (l) {
            return l.code === currentLang;
        });
        var btn = document.createElement("button");
        btn.className = "lang-switcher-btn";
        btn.textContent = currentItem ? currentItem.label : "Language";
        container.appendChild(btn);

        var dropdown = document.createElement("div");
        dropdown.className = "lang-switcher-dropdown";

        langs.forEach(function (lang) {
            var link = document.createElement("a");
            link.href = switchUrl(lang.code);
            link.textContent = lang.label;
            if (lang.code === currentLang) {
                link.className = "active";
            }
            dropdown.appendChild(link);
        });

        container.appendChild(dropdown);

        btn.addEventListener("click", function (e) {
            e.stopPropagation();
            dropdown.classList.toggle("open");
        });

        document.addEventListener("click", function () {
            dropdown.classList.remove("open");
        });

        var menuBar = document.querySelector(".right-buttons");
        if (menuBar) {
            menuBar.insertBefore(container, menuBar.firstChild);
        }
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", createSwitcher);
    } else {
        createSwitcher();
    }
})();
