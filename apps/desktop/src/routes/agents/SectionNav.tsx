import { Icon } from "../../components/common";
import type { DetailSection } from "./detailModel";
import { jumpToSection } from "./hooks";

/**
 * The detail page's sections, beside it on a wide window, marking the one
 * being read.
 *
 * Buttons rather than links: the window's address is its hash route, and a
 * `#section` link would replace the route instead of scrolling. The current
 * section carries `aria-current="location"`, which is what it is — where the
 * reader is on this page — and a bar in the accent gradient marks it. Below the wide
 * layout there is no room beside the page, and the stylesheet hides it.
 */
export function SectionNav({
  sections,
  active,
}: {
  sections: readonly DetailSection[];
  active: string | null;
}) {
  return (
    <nav className="section-nav" aria-label="On this page">
      <p className="section-nav-title" aria-hidden="true">
        On this page
      </p>
      <ol className="section-nav-list">
        {sections.map((section) => (
          <li key={section.id}>
            <button
              type="button"
              className="section-link"
              aria-current={active === section.id ? "location" : undefined}
              onClick={() => jumpToSection(section.id)}
            >
              <Icon name={section.icon} size={15} />
              {section.label}
            </button>
          </li>
        ))}
      </ol>
    </nav>
  );
}
