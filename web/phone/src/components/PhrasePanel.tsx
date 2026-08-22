import { useEffect, useMemo, useState, type JSX } from 'preact/compat'
import { IconClear, IconPhrase } from './icons'
import styles from './PhrasePanel.module.css'

function cx(...names: (string | false | null | undefined)[]): string {
  return names.filter(Boolean).join(' ')
}

const LS_KEY = 'lantype_phrase_groups'

interface Phrase {
  id: string
  title: string
  content: string
}

interface PhraseGroup {
  id: string
  name: string
  phrases: Phrase[]
}

interface PhrasePanelProps {
  sendType: (text: string) => void
  isConnected: boolean
}

function genId(prefix: string): string {
  return prefix + '-' + Date.now().toString(36) + Math.random().toString(36).slice(2, 6)
}

function normalizeGroups(value: unknown): PhraseGroup[] {
  if (!Array.isArray(value)) return []
  return value
    .map((group): PhraseGroup | null => {
      if (!group || typeof group !== 'object') return null
      const raw = group as Partial<PhraseGroup>
      if (typeof raw.id !== 'string' || typeof raw.name !== 'string') return null
      const phrases = Array.isArray(raw.phrases)
        ? raw.phrases
            .map((phrase): Phrase | null => {
              if (!phrase || typeof phrase !== 'object') return null
              const p = phrase as Partial<Phrase>
              if (typeof p.id !== 'string' || typeof p.content !== 'string') return null
              return { id: p.id, title: typeof p.title === 'string' ? p.title : '', content: p.content }
            })
            .filter((phrase): phrase is Phrase => Boolean(phrase))
        : []
      return { id: raw.id, name: raw.name, phrases }
    })
    .filter((group): group is PhraseGroup => Boolean(group))
}

function loadGroups(): PhraseGroup[] {
  try {
    const raw = localStorage.getItem(LS_KEY)
    if (!raw) return []
    return normalizeGroups(JSON.parse(raw))
  } catch {
    return []
  }
}

export function PhrasePanel({ sendType, isConnected }: PhrasePanelProps) {
  const [groups, setGroups] = useState<PhraseGroup[]>(loadGroups)
  const [open, setOpen] = useState(false)
  const [editMode, setEditMode] = useState(false)
  const [selectedGroupId, setSelectedGroupId] = useState(() => groups[0]?.id ?? '')
  const [groupName, setGroupName] = useState('')
  const [phraseTitle, setPhraseTitle] = useState('')
  const [phraseContent, setPhraseContent] = useState('')
  const [editingPhraseId, setEditingPhraseId] = useState<string | null>(null)

  useEffect(() => {
    try {
      localStorage.setItem(LS_KEY, JSON.stringify(groups))
    } catch { /* ignore */ }
  }, [groups])

  useEffect(() => {
    if (selectedGroupId && groups.some((group) => group.id === selectedGroupId)) return
    setSelectedGroupId(groups[0]?.id ?? '')
  }, [groups, selectedGroupId])

  const selectedGroup = useMemo(
    () => groups.find((group) => group.id === selectedGroupId) ?? null,
    [groups, selectedGroupId],
  )

  const saveGroup = (e: JSX.TargetedEvent<HTMLFormElement>) => {
    e.preventDefault()
    const name = groupName.trim()
    if (!name) return
    const group = { id: genId('pg'), name, phrases: [] }
    setGroups((prev) => [...prev, group])
    setSelectedGroupId(group.id)
    setGroupName('')
  }

  const deleteGroup = (id: string) => {
    setGroups((prev) => prev.filter((group) => group.id !== id))
    if (selectedGroupId === id) {
      setEditingPhraseId(null)
      setPhraseTitle('')
      setPhraseContent('')
    }
  }

  const startEditPhrase = (phrase: Phrase) => {
    setEditingPhraseId(phrase.id)
    setPhraseTitle(phrase.title)
    setPhraseContent(phrase.content)
  }

  const clearPhraseDraft = () => {
    setEditingPhraseId(null)
    setPhraseTitle('')
    setPhraseContent('')
  }

  const savePhrase = (e: JSX.TargetedEvent<HTMLFormElement>) => {
    e.preventDefault()
    if (!selectedGroup) return
    const content = phraseContent.trim()
    if (!content) return
    const title = phraseTitle.trim()
    setGroups((prev) => prev.map((group) => {
      if (group.id !== selectedGroup.id) return group
      if (editingPhraseId) {
        return {
          ...group,
          phrases: group.phrases.map((phrase) => (
            phrase.id === editingPhraseId ? { ...phrase, title, content } : phrase
          )),
        }
      }
      return { ...group, phrases: [...group.phrases, { id: genId('ph'), title, content }] }
    }))
    clearPhraseDraft()
  }

  const deletePhrase = (id: string) => {
    if (!selectedGroup) return
    setGroups((prev) => prev.map((group) => (
      group.id === selectedGroup.id
        ? { ...group, phrases: group.phrases.filter((phrase) => phrase.id !== id) }
        : group
    )))
    if (editingPhraseId === id) clearPhraseDraft()
  }

  return (
    <div className={styles.area}>
      <button
        className={cx(styles.trigger, open && styles.triggerActive)}
        onClick={() => setOpen((value) => !value)}
        aria-label="常用语"
        type="button"
      >
        <IconPhrase size={20} />
      </button>

      <div
        className={cx(styles.backdrop, open && styles.backdropOpen)}
        onClick={() => { setOpen(false); setEditMode(false); clearPhraseDraft() }}
      />

      {open && (
        <div className={styles.popover}>
          <div className={styles.head}>
            <span className={styles.title}>常用语</span>
            <button
              className={cx(styles.editToggle, editMode && styles.editToggleActive)}
              onClick={() => { setEditMode((value) => !value); clearPhraseDraft() }}
              type="button"
            >
              {editMode ? '完成' : '管理'}
            </button>
          </div>

          {groups.length > 0 && (
            <div className={styles.groupTabs}>
              {groups.map((group) => (
                <button
                  key={group.id}
                  className={cx(styles.groupTab, selectedGroupId === group.id && styles.groupTabActive)}
                  onClick={() => { setSelectedGroupId(group.id); clearPhraseDraft() }}
                  type="button"
                >
                  <span className={styles.groupName}>{group.name}</span>
                  {editMode && (
                    <span
                      className={styles.groupDelete}
                      onClick={(e) => { e.stopPropagation(); deleteGroup(group.id) }}
                    >
                      <IconClear size={11} />
                    </span>
                  )}
                </button>
              ))}
            </div>
          )}

          {editMode && (
            <form className={styles.groupForm} onSubmit={saveGroup}>
              <input
                className={styles.textInput}
                value={groupName}
                onInput={(e) => setGroupName((e.target as HTMLInputElement).value)}
                placeholder="新分组名称"
                aria-label="新分组名称"
              />
              <button className={styles.smallSave} type="submit">创建分组</button>
            </form>
          )}

          {!selectedGroup ? (
            <div className={styles.empty}>先创建分组</div>
          ) : editMode ? (
            <div className={styles.manager}>
              <div className={styles.phraseList}>
                {selectedGroup.phrases.map((phrase) => (
                  <button
                    key={phrase.id}
                    className={styles.phraseRow}
                    onClick={() => startEditPhrase(phrase)}
                    type="button"
                  >
                    <span className={styles.phraseText}>{phrase.title || phrase.content}</span>
                    <span
                      className={styles.phraseDelete}
                      onClick={(e) => { e.stopPropagation(); deletePhrase(phrase.id) }}
                    >
                      <IconClear size={12} />
                    </span>
                  </button>
                ))}
              </div>
              <form className={styles.phraseForm} onSubmit={savePhrase}>
                <input
                  className={styles.textInput}
                  value={phraseTitle}
                  onInput={(e) => setPhraseTitle((e.target as HTMLInputElement).value)}
                  placeholder="标题，可选"
                  aria-label="常用语标题"
                />
                <textarea
                  className={styles.textarea}
                  value={phraseContent}
                  onInput={(e) => setPhraseContent((e.target as HTMLTextAreaElement).value)}
                  placeholder="常用语内容"
                  aria-label="常用语内容"
                />
                <div className={styles.actions}>
                  {editingPhraseId && (
                    <button className={styles.cancel} type="button" onClick={clearPhraseDraft}>取消</button>
                  )}
                  <button className={styles.save} type="submit" disabled={!phraseContent.trim()}>
                    {editingPhraseId ? '保存' : '新增常用语'}
                  </button>
                </div>
              </form>
            </div>
          ) : selectedGroup.phrases.length > 0 ? (
            <div className={styles.sendList}>
              {selectedGroup.phrases.map((phrase) => (
                <button
                  key={phrase.id}
                  className={styles.sendItem}
                  onClick={() => { sendType(phrase.content); setOpen(false) }}
                  disabled={!isConnected}
                  type="button"
                >
                  <span className={styles.sendTitle}>{phrase.title || phrase.content}</span>
                  {phrase.title && <span className={styles.sendContent}>{phrase.content}</span>}
                </button>
              ))}
            </div>
          ) : (
            <div className={styles.empty}>该分组还没有常用语</div>
          )}
        </div>
      )}
    </div>
  )
}
