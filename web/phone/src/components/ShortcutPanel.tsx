import { useState, useEffect } from 'preact/compat'
import type { Shortcut } from '../hooks/useWebSocket'
import { IconStar, IconArrowUp, IconArrowDown, IconArrowLeft, IconArrowRight, IconClear } from './icons'
import styles from './ShortcutPanel.module.css'

function cx(...names: (string | false | null | undefined)[]): string {
  return names.filter(Boolean).join(' ')
}

const LS_KEY = 'lantype_shortcuts'

const MODIFIER_OPTIONS: { value: string; label: string }[] = [
  { value: 'ctrl', label: 'Ctrl' },
  { value: 'shift', label: 'Shift' },
  { value: 'alt', label: 'Alt' },
  { value: 'meta', label: 'Meta' },
]

const KEY_OPTIONS: { value: string; label: string }[] = [
  ...'abcdefghijklmnopqrstuvwxyz'.split('').map((c) => ({ value: c, label: c.toUpperCase() })),
  ...'0123456789'.split('').map((d) => ({ value: d, label: d })),
  { value: 'enter', label: 'Enter' },
  { value: 'esc', label: 'ESC' },
  { value: 'tab', label: 'Tab' },
  { value: 'space', label: 'Space' },
  { value: 'backspace', label: 'Backspace' },
  { value: 'delete', label: 'Delete' },
  { value: 'home', label: 'Home' },
  { value: 'end', label: 'End' },
  { value: 'pageup', label: 'Page Up' },
  { value: 'pagedown', label: 'Page Down' },
  { value: 'up', label: '↑' },
  { value: 'down', label: '↓' },
  { value: 'left', label: '←' },
  { value: 'right', label: '→' },
  ...Array.from({ length: 12 }, (_, i) => ({ value: 'f' + (i + 1), label: 'F' + (i + 1) })),
]

function modLabel(m: string): string {
  return MODIFIER_OPTIONS.find((o) => o.value === m)?.label ?? m
}

function keyLabel(value: string): string {
  if (value.length === 1) return value.toUpperCase()
  return KEY_OPTIONS.find((o) => o.value === value)?.label ?? value.toUpperCase()
}

function shortcutLabel(s: { modifiers: string[]; key: string }): string {
  const mods = s.modifiers.map(modLabel).join('+')
  return mods ? `${mods}+${keyLabel(s.key)}` : keyLabel(s.key)
}

function genId(): string {
  return 'sc-' + Date.now().toString(36) + Math.random().toString(36).slice(2, 6)
}

function DEFAULT_SHORTCUTS(): Shortcut[] {
  const mk = (modifiers: string[], key: string): Shortcut => ({ id: genId(), modifiers, key })
  return [
    mk(['ctrl'], 'c'),
    mk(['ctrl'], 'v'),
    mk(['ctrl'], 'x'),
    mk(['ctrl'], 'z'),
    mk(['ctrl'], 'a'),
    mk([], 'enter'),
    mk([], 'tab'),
    mk([], 'esc'),
  ]
}

function loadShortcuts(): Shortcut[] {
  try {
    const raw = localStorage.getItem(LS_KEY)
    if (raw) {
      const parsed = JSON.parse(raw)
      if (Array.isArray(parsed) && parsed.length > 0) return parsed
    }
  } catch { /* fall through to defaults */ }
  return DEFAULT_SHORTCUTS()
}

interface ShortcutPanelProps {
  sendKeys: (modifiers: string[], key: string) => void
  isConnected: boolean
}

export function ShortcutPanel({ sendKeys, isConnected }: ShortcutPanelProps) {
  const [shortcuts, setShortcuts] = useState<Shortcut[]>(loadShortcuts)
  const [chipOpen, setChipOpen] = useState(false)
  const [editMode, setEditMode] = useState(false)
  const [draft, setDraft] = useState<{ modifiers: string[]; key: string }>({ modifiers: [], key: 'enter' })
  const [editingId, setEditingId] = useState<string | null>(null)

  useEffect(() => {
    try {
      localStorage.setItem(LS_KEY, JSON.stringify(shortcuts))
    } catch { /* ignore */ }
  }, [shortcuts])

  const persistDraft = () => {
    if (!draft.key) return
    if (editingId && editingId !== '__new__') {
      setShortcuts((prev) => prev.map((s) => (s.id === editingId ? { ...s, ...draft } : s)))
    } else {
      setShortcuts((prev) => [...prev, { id: genId(), ...draft }])
    }
    setDraft({ modifiers: [], key: 'enter' })
    setEditingId(null)
  }

  const toggleModifier = (value: string) => {
    setDraft((d) => ({
      ...d,
      modifiers: d.modifiers.includes(value) ? d.modifiers.filter((m) => m !== value) : [...d.modifiers, value],
    }))
  }

  return (
    <div className={styles.chipArea}>
      <button
        className={cx(styles.chipBtn, chipOpen && styles.chipBtnActive)}
        onClick={() => setChipOpen((o) => !o)}
        aria-label="快捷键片段"
        type="button"
      >
        <IconStar size={20} />
      </button>

      <div className={styles.arrowCluster}>
        <button
          className={cx(styles.arrowBtn, styles.arrowUp)}
          onClick={() => sendKeys([], 'up')}
          disabled={!isConnected}
          aria-label="上"
          type="button"
        >
          <IconArrowUp size={16} />
        </button>
        <button
          className={cx(styles.arrowBtn, styles.arrowLeft)}
          onClick={() => sendKeys([], 'left')}
          disabled={!isConnected}
          aria-label="左"
          type="button"
        >
          <IconArrowLeft size={16} />
        </button>
        <button
          className={cx(styles.arrowBtn, styles.arrowDown)}
          onClick={() => sendKeys([], 'down')}
          disabled={!isConnected}
          aria-label="下"
          type="button"
        >
          <IconArrowDown size={16} />
        </button>
        <button
          className={cx(styles.arrowBtn, styles.arrowRight)}
          onClick={() => sendKeys([], 'right')}
          disabled={!isConnected}
          aria-label="右"
          type="button"
        >
          <IconArrowRight size={16} />
        </button>
      </div>

      <div
        className={cx(styles.chipBackdrop, chipOpen && styles.chipBackdropOpen)}
        onClick={() => { setChipOpen(false); setEditMode(false) }}
      />

      {chipOpen && (
        <div className={styles.chipPopover}>
          <div className={styles.chipHead}>
            <span className={styles.chipTitle}>快捷键片段</span>
            <button
              className={cx(styles.chipEditToggle, editMode && styles.chipEditToggleActive)}
              onClick={() => setEditMode((m) => !m)}
              aria-pressed={editMode}
              type="button"
            >
              {editMode ? '完成' : '编辑'}
            </button>
          </div>

          {editingId === null ? (
            <div className={styles.chipGrid}>
              {shortcuts.map((s) => (
                <div className={styles.chipItem} key={s.id}>
                  <button
                    className={cx(styles.chip, editMode && styles.chipEditable)}
                    onClick={() => {
                      if (editMode) {
                        setDraft({ modifiers: s.modifiers, key: s.key })
                        setEditingId(s.id)
                      } else {
                        sendKeys(s.modifiers, s.key)
                        setChipOpen(false)
                      }
                    }}
                    disabled={!editMode && !isConnected}
                    type="button"
                  >
                    <span className={styles.chipLabel}>{shortcutLabel(s)}</span>
                  </button>
                  {editMode && (
                    <button
                      className={styles.chipDelete}
                      onClick={() => setShortcuts((prev) => prev.filter((x) => x.id !== s.id))}
                      aria-label={`删除快捷键 ${shortcutLabel(s)}`}
                      type="button"
                    >
                      <IconClear size={12} />
                    </button>
                  )}
                </div>
              ))}
              {editMode && (
                <button
                  className={cx(styles.chip, styles.chipAdd)}
                  onClick={() => setEditingId('__new__')}
                  type="button"
                >
                  ＋ 新增
                </button>
              )}
            </div>
          ) : (
            <div className={styles.chipEditor}>
              <div className={styles.chipEditorLabel}>修饰键</div>
              <div className={styles.chipModifiers}>
                {MODIFIER_OPTIONS.map((o) => (
                  <button
                    key={o.value}
                    className={cx(styles.modKey, draft.modifiers.includes(o.value) && styles.modKeyActive)}
                    onClick={() => toggleModifier(o.value)}
                    aria-pressed={draft.modifiers.includes(o.value)}
                    type="button"
                  >
                    {o.label}
                  </button>
                ))}
              </div>
              <div className={styles.chipEditorLabel}>按键</div>
              <select
                className={styles.chipKeySelect}
                value={draft.key}
                onChange={(e) => setDraft((d) => ({ ...d, key: (e.target as HTMLSelectElement).value }))}
                aria-label="快捷键按键"
              >
                {KEY_OPTIONS.map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label}
                  </option>
                ))}
              </select>
              <div className={styles.chipEditorActions}>
                <button className={styles.chipSave} onClick={persistDraft} type="button">保存</button>
                <button
                  className={styles.chipCancel}
                  onClick={() => {
                    setDraft({ modifiers: [], key: 'enter' })
                    setEditingId(null)
                  }}
                  type="button"
                >
                  取消
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
