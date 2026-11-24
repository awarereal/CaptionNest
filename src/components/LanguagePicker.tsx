// Searchable language list (cmdk inside a Radix popover). Each option can carry a
// quality badge and a short note about known limitations.
import { useMemo, useState, type ReactNode } from "react";
import { Popover } from "radix-ui";
import { Command } from "cmdk";
import { CaretDown, Check, MagnifyingGlass } from "@phosphor-icons/react";

export interface PickerItem {
  value: string;
  label: string;
  native?: string;
  /** Extra words that should match when searching (e.g. "Bengali" for Bangla). */
  keywords?: string[];
  badge?: ReactNode;
  meta?: ReactNode;
  group: string;
  disabled?: boolean;
}

export function LanguagePicker({
  id,
  value,
  items,
  onChange,
  label,
  placeholder = "Search languages",
}: {
  id: string;
  value: string;
  items: PickerItem[];
  onChange: (value: string) => void;
  label: string;
  placeholder?: string;
}) {
  const [open, setOpen] = useState(false);
  const current = items.find((i) => i.value === value);
  const groups = useMemo(() => {
    const map = new Map<string, PickerItem[]>();
    for (const item of items) {
      if (!map.has(item.group)) map.set(item.group, []);
      map.get(item.group)!.push(item);
    }
    return [...map.entries()];
  }, [items]);

  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild>
        <button id={id} className="field field--select picker__trigger" aria-label={`${label}: ${current?.label ?? value}`}>
          <span className="picker__value">
            <span>{current?.label ?? value}</span>
            {current?.native && current.native !== current.label && <span className="picker__native caption-text">{current.native}</span>}
          </span>
          {current?.badge}
          <span className="field__caret">
            <CaretDown size={14} />
          </span>
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content className="menu picker" align="start" sideOffset={6} collisionPadding={12}>
          <Command
            label={label}
            filter={(itemValue, search, keywords) => {
              const hay = `${itemValue} ${(keywords ?? []).join(" ")}`.toLowerCase();
              return hay.includes(search.toLowerCase().trim()) ? 1 : 0;
            }}
          >
            <div className="picker__search">
              <MagnifyingGlass size={16} />
              <Command.Input placeholder={placeholder} autoFocus />
            </div>
            <Command.List className="picker__list">
              <Command.Empty className="picker__empty">No language matches.</Command.Empty>
              {groups.map(([group, list]) => (
                <Command.Group key={group} heading={group} className="picker__group">
                  {list.map((item) => (
                    <Command.Item
                      key={item.value}
                      value={`${item.label} ${item.value}`}
                      keywords={[item.native ?? "", ...(item.keywords ?? [])]}
                      disabled={item.disabled}
                      onSelect={() => {
                        onChange(item.value);
                        setOpen(false);
                      }}
                      className="menu__item picker__item"
                    >
                      <span className="menu__check">{item.value === value && <Check size={14} weight="bold" />}</span>
                      <span className="picker__names">
                        <span>{item.label}</span>
                        {item.native && item.native !== item.label && <span className="picker__native caption-text">{item.native}</span>}
                      </span>
                      {item.meta && <span className="picker__meta">{item.meta}</span>}
                      {item.badge}
                    </Command.Item>
                  ))}
                </Command.Group>
              ))}
            </Command.List>
          </Command>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
