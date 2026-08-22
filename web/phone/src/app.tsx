import { useState, useEffect, useRef, useCallback, type JSX } from 'preact/compat'
import { useWebSocket } from './hooks/useWebSocket'
import { ShortcutPanel } from './components/ShortcutPanel'
import { PhrasePanel } from './components/PhrasePanel'
import { SettingsPanel } from './components/SettingsPanel'
import { IconGear, IconEnter, IconSend, IconClear } from './components/icons'
import styles from './app.module.css'

function cx(...names: (string | false | null | undefined)[]): string {
  return names.filter(Boolean).join(' ')
}

const NICKNAME_KEY = 'lantype_nickname'

function loadNickname(): string {
  try {
    return localStorage.getItem(NICKNAME_KEY) || ''
  } catch { return '' }
}

export function App() {
  const nicknameRef = useRef(loadNickname())
  const { status, connectedDevice, errorMessage, sendDiff, sendKeys, sendHello, sendType, resetPrev } = useWebSocket(nicknameRef)
  const [nickname, setNickname] = useState(nicknameRef.current)
  const [text, setText] = useState('')
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [autoSync, setAutoSync] = useState(false)
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const isComposing = useRef(false)

  useEffect(() => {
    try {
      localStorage.setItem(NICKNAME_KEY, nickname)
    } catch { /* ignore */ }
  }, [nickname])

  const handleNickname = useCallback((e: JSX.TargetedEvent<HTMLInputElement>) => {
    const v = (e.target as HTMLInputElement).value
    nicknameRef.current = v
    setNickname(v)
    sendHello()
  }, [sendHello])

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

  const handlePaste = useCallback((e: JSX.TargetedClipboardEvent<HTMLTextAreaElement>) => {
    const pasted = e.clipboardData?.getData('text') ?? ''
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
    if (autoSync) {
      // In auto-sync the text is already on the receiver; Enter commits the
      // current line, then clear the local buffer for the next input.
      setText('')
      resetPrev()
    }
    textareaRef.current?.focus()
  }, [sendType, autoSync, resetPrev])

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
            <IconGear size={20} />
          </button>
        </div>
        <div className={styles.headerRight}>
          <button
            className={cx(styles.toggle, autoSync && styles.toggleActive)}
            onClick={() => setAutoSync(!autoSync)}
            role="switch"
            aria-checked={autoSync}
            aria-label="自动发送"
            type="button"
          >
            <span className={styles.toggleText}>自动发送</span>
            <span
              className={cx(styles.toggleKnob, autoSync && styles.toggleKnobActive)}
            />
          </button>
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
          <IconEnter size={22} />
        </button>
        {(!autoSync || !hasText) && (
          <button
            className={cx(styles.btn, styles.btnSend, autoSync && styles.hidden)}
            onClick={handleSend}
            disabled={!isConnected || !hasText}
            aria-label="发送"
          >
            <IconSend size={22} />
          </button>
        )}
        {hasText && (
          <button
            className={cx(styles.btn, styles.btnClear)}
            onClick={handleClear}
            aria-label="清空"
          >
            <IconClear size={20} />
          </button>
        )}
      </div>
      <div className={styles.footer}>文字实时同步到桌面端</div>

      <ShortcutPanel sendKeys={sendKeys} isConnected={isConnected} />
      <PhrasePanel sendType={sendType} isConnected={isConnected} />

      <SettingsPanel
        open={settingsOpen}
        nickname={nickname}
        onNickname={handleNickname}
        onClose={() => setSettingsOpen(false)}
      />
    </div>
  )
}
