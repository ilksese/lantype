import { useState, useEffect, useRef, useCallback, type JSX } from 'preact/compat'
import type { ServerMessage, ClientStatus, Shortcut } from './types'
import styles from './app.module.css'

function cx(...names: (string | false | null | undefined)[]): string {
  return names.filter(Boolean).join(' ')
}

function getFriendlyName(): string {
  const ua = navigator.userAgent
  let browser = '浏览器'
  if (ua.includes('Chrome') && !ua.includes('Edg')) browser = 'Chrome'
  else if (ua.includes('Safari') && !ua.includes('Chrome')) browser = 'Safari'
  else if (ua.includes('Edg')) browser = 'Edge'
  else if (ua.includes('Firefox')) browser = 'Firefox'
  let os = '未知设备'
  if (ua.includes('iPhone') || ua.includes('iPad')) os = 'iOS'
  else if (ua.includes('Android')) os = 'Android'
  else if (ua.includes('Mac')) os = 'macOS'
  else if (ua.includes('Windows')) os = 'Windows'
  else if (ua.includes('Linux')) os = 'Linux'
  return browser + ' · ' + os
}

function useWebSocket() {
  const HEARTBEAT_INTERVAL_MS = 15000
  const PONG_TIMEOUT_MS = 5000
  const RECONNECT_DELAY_MS = 3000

  const [status, setStatus] = useState<ClientStatus>('connecting')
  const [connectedDevice, setConnectedDevice] = useState<string | null>(null)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const wsRef = useRef<WebSocket | null>(null)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const heartbeatRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const pongTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const prevTextRef = useRef('')

  const clearReconnectTimer = useCallback(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current)
      timerRef.current = null
    }
  }, [])

  const clearHeartbeat = useCallback(() => {
    if (heartbeatRef.current) {
      clearInterval(heartbeatRef.current)
      heartbeatRef.current = null
    }
    if (pongTimeoutRef.current) {
      clearTimeout(pongTimeoutRef.current)
      pongTimeoutRef.current = null
    }
  }, [])

  const closeCurrentSocket = useCallback(() => {
    const ws = wsRef.current
    wsRef.current = null
    if (!ws) return
    ws.onopen = null
    ws.onmessage = null
    ws.onclose = null
    ws.onerror = null
    if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) {
      ws.close()
    }
  }, [])

  const sendMessage = useCallback((msg: object) => {
    const ws = wsRef.current
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(msg))
    }
  }, [])

  const sendDiff = useCallback((newText: string) => {
    const prev = prevTextRef.current
    let commonLen = 0
    while (
      commonLen < prev.length &&
      commonLen < newText.length &&
      prev[commonLen] === newText[commonLen]
    ) {
      commonLen++
    }
    const backspace = prev.length - commonLen
    const append = newText.substring(commonLen)
    if (backspace > 0 || append.length > 0) {
      sendMessage({ type: 'diff', backspace, text: append })
    }
    prevTextRef.current = newText
  }, [sendMessage])

  const sendType = useCallback((text: string) => {
    sendMessage({ type: 'type', text })
  }, [sendMessage])

  const sendKeys = useCallback((modifiers: string[], key: string) => {
    sendMessage({ type: 'keys', modifiers, key })
  }, [sendMessage])

  const connect = useCallback((force = false) => {
    const current = wsRef.current
    if (!force && current && (current.readyState === WebSocket.OPEN || current.readyState === WebSocket.CONNECTING)) {
      return
    }

    clearReconnectTimer()
    clearHeartbeat()
    closeCurrentSocket()

    const params = new URLSearchParams(window.location.search)
    const wsPort = params.get('ws') || '0'
    const pin = params.get('pin') || ''
    const host = window.location.hostname
    const url = 'ws://' + host + ':' + wsPort + (pin ? '?pin=' + encodeURIComponent(pin) : '')

    setStatus('reconnecting')
    const ws = new WebSocket(url)
    wsRef.current = ws

    const sendPing = () => {
      if (document.visibilityState === 'hidden') return
      if (wsRef.current !== ws || ws.readyState !== WebSocket.OPEN) return
      ws.send(JSON.stringify({ type: 'ping' }))
      if (pongTimeoutRef.current) clearTimeout(pongTimeoutRef.current)
      pongTimeoutRef.current = setTimeout(() => {
        if (wsRef.current === ws && ws.readyState === WebSocket.OPEN) {
          ws.close()
        }
      }, PONG_TIMEOUT_MS)
    }

    ws.onopen = () => {
      if (wsRef.current !== ws) return
      setStatus('connected')
      ws.send(JSON.stringify({ type: 'hello', device_name: getFriendlyName() }))
      clearHeartbeat()
      heartbeatRef.current = setInterval(sendPing, HEARTBEAT_INTERVAL_MS)
      sendPing()
    }

    ws.onmessage = (e: MessageEvent) => {
      if (wsRef.current !== ws) return
      try {
        const msg: ServerMessage = JSON.parse(e.data)
        if (msg.type === 'pong') {
          if (pongTimeoutRef.current) {
            clearTimeout(pongTimeoutRef.current)
            pongTimeoutRef.current = null
          }
        }
        if (msg.type === 'connected' && msg.device) {
          setConnectedDevice(msg.device)
        }
        if (msg.type === 'error' && msg.message) {
          setErrorMessage(msg.message)
        }
      } catch { /* ignore */ }
    }

    ws.onclose = (e: CloseEvent) => {
      if (wsRef.current !== ws) return
      wsRef.current = null
      clearHeartbeat()
      if (e.code === 1008) {
        setStatus('blacklisted')
        return
      }
      setStatus('disconnected')
      clearReconnectTimer()
      timerRef.current = setTimeout(() => connect(true), RECONNECT_DELAY_MS)
    }

    ws.onerror = () => {
      ws.close()
    }
  }, [clearHeartbeat, clearReconnectTimer, closeCurrentSocket])

  const ensureConnected = useCallback(() => {
    if (document.visibilityState === 'hidden' || status === 'blacklisted') return
    const ws = wsRef.current
    if (ws && ws.readyState === WebSocket.CONNECTING) return
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      connect(true)
      return
    }
    ws.send(JSON.stringify({ type: 'ping' }))
    if (pongTimeoutRef.current) clearTimeout(pongTimeoutRef.current)
    pongTimeoutRef.current = setTimeout(() => {
      if (wsRef.current === ws && ws.readyState === WebSocket.OPEN) {
        ws.close()
      }
    }, PONG_TIMEOUT_MS)
  }, [connect, status])

  useEffect(() => {
    connect()
    return () => {
      clearReconnectTimer()
      clearHeartbeat()
      closeCurrentSocket()
    }
  }, [clearHeartbeat, clearReconnectTimer, closeCurrentSocket, connect])

  useEffect(() => {
    const onVisible = () => {
      if (document.visibilityState === 'visible') ensureConnected()
    }
    const onPageShow = () => ensureConnected()

    document.addEventListener('visibilitychange', onVisible)
    window.addEventListener('pageshow', onPageShow)
    return () => {
      document.removeEventListener('visibilitychange', onVisible)
      window.removeEventListener('pageshow', onPageShow)
    }
  }, [ensureConnected])

  return { status, connectedDevice, errorMessage, sendDiff, sendKeys, sendType: useCallback((text: string) => {
    sendMessage({ type: 'type', text })
    prevTextRef.current = ''
  }, [sendMessage]), resetPrev: useCallback(() => { prevTextRef.current = '' }, []) }
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

export function App() {
  const { status, connectedDevice, errorMessage, sendDiff, sendKeys, sendType, resetPrev } = useWebSocket()
  const [text, setText] = useState('')
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [autoSync, setAutoSync] = useState(false)
  const [shortcuts, setShortcuts] = useState<Shortcut[]>(loadShortcuts)
  const [chipOpen, setChipOpen] = useState(false)
  const [editMode, setEditMode] = useState(false)
  const [draft, setDraft] = useState<{ modifiers: string[]; key: string }>({ modifiers: [], key: 'enter' })
  const [editingId, setEditingId] = useState<string | null>(null)
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const isComposing = useRef(false)

  useEffect(() => {
    try {
      localStorage.setItem(LS_KEY, JSON.stringify(shortcuts))
    } catch { /* ignore */ }
  }, [shortcuts])

  const persistDraft = useCallback(() => {
    if (!draft.key) return
    if (editingId && editingId !== '__new__') {
      setShortcuts((prev) => prev.map((s) => (s.id === editingId ? { ...s, ...draft } : s)))
    } else {
      setShortcuts((prev) => [...prev, { id: genId(), ...draft }])
    }
    setDraft({ modifiers: [], key: 'enter' })
    setEditingId(null)
  }, [draft, editingId])

  const toggleModifier = useCallback((value: string) => {
    setDraft((d) => ({
      ...d,
      modifiers: d.modifiers.includes(value) ? d.modifiers.filter((m) => m !== value) : [...d.modifiers, value],
    }))
  }, [])

  useEffect(() => {
    if (errorMessage) {
      if (toastTimer.current) clearTimeout(toastTimer.current)
      toastTimer.current = setTimeout(() => {
        // clear via reconnect/reset handled by hook
      }, 5000)
    }
    return () => {
      if (toastTimer.current) clearTimeout(toastTimer.current)
    }
  }, [errorMessage])

  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const isConnected = status === 'connected'
  const hasText = text.length > 0

  const handleInput = useCallback((e: JSX.TargetedEvent<HTMLTextAreaElement>) => {
    const value = (e.target as HTMLTextAreaElement).value
    setText(value)
    if (!isComposing.current && autoSync) sendDiff(value)
  }, [autoSync, sendDiff])

  const handlePaste = useCallback((e: JSX.TargetedEvent<HTMLTextAreaElement>) => {
    const clip = (e.nativeEvent ?? e) as ClipboardEvent
    const pasted = clip.clipboardData?.getData('text') ?? ''
    if (!pasted) return
    e.preventDefault()
    const el = e.target as HTMLTextAreaElement
    const start = el.selectionStart ?? el.value.length
    const end = el.selectionEnd ?? el.value.length
    const value = el.value.slice(0, start) + pasted + el.value.slice(end)
    setText(value)
    if (!isComposing.current && autoSync) sendDiff(value)
    requestAnimationFrame(() => {
      el.focus()
      const pos = start + pasted.length
      el.setSelectionRange(pos, pos)
    })
  }, [autoSync, sendDiff])

  const handleCompositionStart = useCallback(() => {
    isComposing.current = true
  }, [])

  const handleCompositionEnd = useCallback((e: JSX.TargetedEvent<HTMLTextAreaElement>) => {
    isComposing.current = false
    const value = (e.target as HTMLTextAreaElement).value
    if (autoSync) sendDiff(value)
  }, [autoSync, sendDiff])

  const handleClear = useCallback(() => {
    setText('')
    sendDiff('')
    resetPrev()
    textareaRef.current?.focus()
  }, [sendDiff, resetPrev])

  const handleSend = useCallback(() => {
    if (!text) return
    sendType(text)
    setText('')
    resetPrev()
    textareaRef.current?.focus()
  }, [text, sendType, resetPrev])

  const handleEnter = useCallback(() => {
    sendType('\n')
    textareaRef.current?.focus()
  }, [sendType])

  let statusText = '未连接'
  let deviceText: string | JSX.Element = '扫码或输入地址连接'
  if (status === 'connecting' || status === 'reconnecting') {
    statusText = '连接中...'
    deviceText = '连接中...'
  } else if (status === 'connected') {
    statusText = '已连接'
    deviceText = connectedDevice
      ? <>已连接至 <span className={styles.deviceName}>{connectedDevice}</span></>
      : '已连接'
  } else if (status === 'blacklisted') {
    statusText = '连接被拒绝'
    deviceText = errorMessage || '此设备已被拉黑，无法连接'
  } else if (status === 'disconnected') {
    statusText = '已断开'
    deviceText = '连接已断开，正在重连...'
  }

  return (
    <div className={styles.body}>
      {errorMessage && (
        <div className={styles.toastWrap}>
          <div className={styles.toast} onClick={() => {}}>
            {errorMessage}
          </div>
        </div>
      )}
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <h1 className={styles.title}>LanType</h1>
          <button
            className={styles.btnSettings}
            onClick={() => setSettingsOpen(true)}
            aria-label="设置"
          >
            &#9881;
          </button>
        </div>
        <div
          className={cx(
            styles.status,
            status === 'connected' && styles.statusConnected,
            status === 'disconnected' && styles.statusDisconnected,
            (status === 'reconnecting' || status === 'connecting') && styles.statusReconnecting,
          )}
        >
          <span
            className={cx(
              styles.dot,
              status === 'connected' && styles.dotConnected,
              status === 'disconnected' && styles.dotDisconnected,
              (status === 'reconnecting' || status === 'connecting') && styles.dotReconnecting,
              status === 'blacklisted' && styles.dotDisconnected,
            )}
          />
          <span>{statusText}</span>
        </div>
      </div>
      <div className={styles.deviceRow}>{deviceText}</div>
      <div className={styles.inputWrap}>
        <textarea
          ref={textareaRef}
          className={styles.textarea}
          value={text}
          onInput={handleInput}
          onPaste={handlePaste}
          onCompositionStart={handleCompositionStart}
          onCompositionEnd={handleCompositionEnd}
          placeholder="在此输入文字..."
          disabled={!isConnected && status !== 'blacklisted'}
          autocomplete="off"
          autocorrect="off"
          autocapitalize="off"
          spellcheck={false}
        />
      </div>
      <div className={styles.btnBar}>
        <button
          className={cx(styles.btn, styles.btnEnter)}
          onClick={handleEnter}
          disabled={!isConnected}
          aria-label="回车"
        >
          ↵
        </button>
        {(!autoSync || !hasText) && (
          <button
            className={cx(styles.btn, styles.btnSend, autoSync && styles.hidden)}
            onClick={handleSend}
            disabled={!isConnected || !hasText}
            aria-label="发送"
          >
            ➤
          </button>
        )}
        {hasText && (
          <button
            className={cx(styles.btn, styles.btnClear)}
            onClick={handleClear}
            aria-label="清空"
          >
            ✕
          </button>
        )}
      </div>
      <div className={styles.footer}>文字实时同步到桌面端</div>

      <div className={styles.chipArea}>
        <button
          className={cx(styles.chipBtn, chipOpen && styles.chipBtnActive)}
          onClick={() => setChipOpen((o) => !o)}
          aria-label="快捷键片段"
        >
          <span className={styles.chipIcon}>&#9733;</span>
        </button>

        <div className={styles.arrowCluster}>
          <button
            className={cx(styles.arrowBtn, styles.arrowUp)}
            onClick={() => sendKeys([], 'up')}
            disabled={!isConnected}
            aria-label="上"
          >
            ↑
          </button>
          <button
            className={cx(styles.arrowBtn, styles.arrowLeft)}
            onClick={() => sendKeys([], 'left')}
            disabled={!isConnected}
            aria-label="左"
          >
            ←
          </button>
          <button
            className={cx(styles.arrowBtn, styles.arrowDown)}
            onClick={() => sendKeys([], 'down')}
            disabled={!isConnected}
            aria-label="下"
          >
            ↓
          </button>
          <button
            className={cx(styles.arrowBtn, styles.arrowRight)}
            onClick={() => sendKeys([], 'right')}
            disabled={!isConnected}
            aria-label="右"
          >
            →
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
              >
                {editMode ? '完成' : '编辑'}
              </button>
            </div>

            {editingId === null ? (
              <div className={styles.chipGrid}>
                {shortcuts.map((s) => (
                  <div
                    key={s.id}
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
                  >
                    <span className={styles.chipLabel}>{shortcutLabel(s)}</span>
                    {editMode && (
                      <button
                        className={styles.chipDelete}
                        onClick={(e) => {
                          e.stopPropagation()
                          setShortcuts((prev) => prev.filter((x) => x.id !== s.id))
                        }}
                      >
                        ✕
                      </button>
                    )}
                  </div>
                ))}
                {editMode && (
                  <button className={cx(styles.chip, styles.chipAdd)} onClick={() => setEditingId('__new__')}>
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
                >
                  {KEY_OPTIONS.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
                <div className={styles.chipEditorActions}>
                  <button className={styles.chipSave} onClick={persistDraft}>保存</button>
                  <button
                    className={styles.chipCancel}
                    onClick={() => {
                      setDraft({ modifiers: [], key: 'enter' })
                      setEditingId(null)
                    }}
                  >
                    取消
                  </button>
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      <div
        className={cx(styles.overlay, settingsOpen && styles.overlayOpen)}
        onClick={(e) => { if (e.target === e.currentTarget) setSettingsOpen(false) }}
      >
        <div
          className={styles.settingsPanel}
        >
          <h2 className={styles.settingsTitle}>设置</h2>
          <div className={styles.settingsRow}>
            <span className={styles.settingsLabel}>自动同步</span>
            <div
              className={cx(styles.toggle, autoSync && styles.toggleActive)}
              onClick={() => setAutoSync(!autoSync)}
            >
              <span
                className={cx(styles.toggleKnob, autoSync && styles.toggleKnobActive)}
              />
            </div>
          </div>
          <button
            className={styles.btnCloseSettings}
            onClick={() => setSettingsOpen(false)}
          >
            关闭
          </button>
        </div>
      </div>
    </div>
  )
}
