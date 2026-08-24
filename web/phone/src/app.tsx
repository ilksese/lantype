import { useState, useEffect, useRef, useCallback, type JSX } from 'preact/compat'
import {
  getLocationConnectionTarget,
  hasConnectionCredential,
  isConnectableTarget,
  parseConnectionInput,
  useWebSocket,
  validateConnectionTarget,
  type ConnectionIssue,
  type ConnectionTarget,
} from './hooks/useWebSocket'
import { useInputSync } from './hooks/useInputSync'
import { supportsGraphemeDiff } from './textDiff'
import { ShortcutPanel } from './components/ShortcutPanel'
import { PhrasePanel } from './components/PhrasePanel'
import { SettingsPanel } from './components/SettingsPanel'
import { HistoryPanel, type HistoryItem } from './components/HistoryPanel'
import { IconGear, IconEnter, IconSend, IconClear } from './components/icons'
import styles from './app.module.css'

function cx(...names: (string | false | null | undefined)[]): string {
  return names.filter(Boolean).join(' ')
}

const NICKNAME_KEY = 'lantype_nickname'
const KEEP_AWAKE_KEY = 'lantype_keep_awake'
const HISTORY_KEY = 'lantype_send_history'
const HISTORY_LIMIT = 50

interface WakeLockSentinel extends EventTarget {
  release: () => Promise<void>
}

interface WakeLockNavigator {
  wakeLock: {
    request: (type: 'screen') => Promise<WakeLockSentinel>
  }
}

function loadNickname(): string {
  try {
    return localStorage.getItem(NICKNAME_KEY) || ''
  } catch { return '' }
}

function loadKeepAwake(): boolean {
  try {
    return localStorage.getItem(KEEP_AWAKE_KEY) === '1'
  } catch { return false }
}

function genHistoryId(): string {
  return 'hi-' + Date.now().toString(36) + Math.random().toString(36).slice(2, 6)
}

function normalizeHistory(value: unknown): HistoryItem[] {
  if (!Array.isArray(value)) return []
  return value
    .map((item): HistoryItem | null => {
      if (!item || typeof item !== 'object') return null
      const raw = item as Partial<HistoryItem>
      if (typeof raw.id !== 'string' || typeof raw.text !== 'string' || typeof raw.createdAt !== 'number') return null
      return { id: raw.id, text: raw.text, createdAt: raw.createdAt }
    })
    .filter((item): item is HistoryItem => Boolean(item))
    .slice(0, HISTORY_LIMIT)
}

function loadHistory(): HistoryItem[] {
  try {
    const raw = localStorage.getItem(HISTORY_KEY)
    if (!raw) return []
    return normalizeHistory(JSON.parse(raw))
  } catch { return [] }
}

interface ConnectionPanelProps {
  open: boolean
  target: ConnectionTarget | null
  issue: ConnectionIssue | null
  onConnect: (target: ConnectionTarget) => boolean
  onRetry: () => boolean
  onEdit: (clearCredentials: boolean) => void
  onClose: () => void
}

function targetCredential(target: ConnectionTarget | null): string {
  return target?.token || target?.pin || ''
}

function sameTargetEndpoint(left: ConnectionTarget | null, right: ConnectionTarget | null): boolean {
  if (!left || !right) return false
  return left.scheme === right.scheme && left.host === right.host && left.port === right.port
}

function ConnectionPanel({
  open,
  target,
  issue,
  onConnect,
  onRetry,
  onEdit,
  onClose,
}: ConnectionPanelProps) {
  const locationTarget = getLocationConnectionTarget()
  const initialTarget = target || locationTarget
  const [link, setLink] = useState('')
  const [host, setHost] = useState(initialTarget?.host || '')
  const [port, setPort] = useState(initialTarget?.port ? String(initialTarget.port) : '')
  const [credential, setCredential] = useState(targetCredential(initialTarget))
  const [formError, setFormError] = useState<string | null>(null)
  const dialogRef = useRef<HTMLDialogElement>(null)
  const firstInputRef = useRef<HTMLInputElement>(null)
  const returnFocusRef = useRef<HTMLElement | null>(null)
  const targetKey = target
    ? `${target.scheme}|${target.host}|${target.port}|${target.path || '/'}|${target.token || target.pin || target.session || ''}`
    : 'none'
  const canClose = Boolean(target && isConnectableTarget(target))

  useEffect(() => {
    if (!open) return
    const next = target || getLocationConnectionTarget()
    setLink('')
    setHost(next?.host || '')
    setPort(next?.port ? String(next.port) : '')
    setCredential(targetCredential(next))
    setFormError(null)
  }, [open, targetKey])

  useEffect(() => {
    const dialog = dialogRef.current
    if (!dialog) return
    if (open) {
      if (!dialog.open) {
        returnFocusRef.current = document.activeElement instanceof HTMLElement
          ? document.activeElement
          : null
        dialog.showModal()
      }
      requestAnimationFrame(() => firstInputRef.current?.focus())
      return
    }
    if (dialog.open) dialog.close()
    const returnFocus = returnFocusRef.current
    returnFocusRef.current = null
    requestAnimationFrame(() => returnFocus?.focus())
  }, [open])

  const handleLink = useCallback((event: JSX.TargetedEvent<HTMLInputElement>) => {
    const value = (event.target as HTMLInputElement).value
    setLink(value)
    if (!value.trim()) {
      setFormError(null)
      return
    }

    const result = parseConnectionInput({ link: value, host, port })
    if (result.target) {
      setHost(result.target.host)
      setPort(String(result.target.port))
      const parsedCredential = result.target.token || result.target.pin
      if (parsedCredential) setCredential(parsedCredential)
      else if (target && !sameTargetEndpoint(target, result.target)) setCredential('')
      setFormError(null)
    } else if (result.error && (/^[a-z][a-z\d+.-]*:\/\//i.test(value.trim()) || value.includes('.'))) {
      setFormError(result.error)
    }
  }, [host, port, target])

  const handleSubmit = useCallback((event: JSX.TargetedEvent<HTMLFormElement>) => {
    event.preventDefault()
    const result = parseConnectionInput({
      link: link.trim() || undefined,
      host,
      port,
      credential,
    })
    if (!result.target) {
      setFormError(result.error || '请检查连接地址和端口')
      return
    }

    const shouldKeepSession = Boolean(
      target?.session
      && sameTargetEndpoint(target, result.target)
      && (!credential.trim() || credential.trim() === targetCredential(target)),
    )
    const nextTarget = result.target.session || !shouldKeepSession
      ? result.target
      : { ...result.target, session: target?.session }
    const validationError = validateConnectionTarget(nextTarget)
    if (validationError) {
      setFormError(validationError)
      return
    }
    if (!onConnect(nextTarget)) {
      setFormError('连接未启动，请检查连接信息后重试')
      return
    }
    setFormError(null)
  }, [credential, host, link, onConnect, port, target])

  const handleRescan = useCallback(() => {
    setLink('')
    setCredential('')
    setFormError(null)
    onEdit(true)
  }, [onEdit])

  const visibleError = formError || issue?.message

  return (
    <dialog
      ref={dialogRef}
      className={styles.connectionOverlay}
      aria-labelledby="connection-title"
      onClick={(event) => { if (event.target === event.currentTarget && canClose) onClose() }}
      onCancel={(event) => {
        event.preventDefault()
        if (canClose) onClose()
      }}
    >
      <section className={styles.connectionPanel}>
        <div className={styles.connectionHead}>
          <h2 id="connection-title" className={styles.connectionTitle}>连接桌面端</h2>
          {canClose && (
            <button
              className={styles.connectionClose}
              type="button"
              onClick={onClose}
              aria-label="关闭连接设置"
            >
              ×
            </button>
          )}
        </div>

        {visibleError && (
          <div className={styles.connectionError} role="alert" aria-live="assertive">
            {visibleError}
          </div>
        )}

        <form className={styles.connectionForm} onSubmit={handleSubmit}>
          <label className={styles.connectionLabel}>
            二维码完整链接或地址
            <input
              ref={firstInputRef}
              className={styles.connectionInput}
              value={link}
              onInput={handleLink}
              placeholder="http://电脑地址:端口/?pin=123456"
              autoComplete="off"
              autoCapitalize="off"
              spellcheck={false}
            />
          </label>
          <div className={styles.connectionHint}>也可以手动填写主机、端口和配对信息</div>
          <div className={styles.connectionFields}>
            <label className={styles.connectionLabel}>
              主机地址
              <input
                className={styles.connectionInput}
                value={host}
                onInput={(event) => setHost((event.target as HTMLInputElement).value)}
                placeholder="192.168.1.10"
                autoComplete="off"
                autoCapitalize="off"
                spellcheck={false}
              />
            </label>
            <label className={styles.connectionLabel}>
              端口
              <input
                className={styles.connectionInput}
                value={port}
                onInput={(event) => setPort((event.target as HTMLInputElement).value)}
                placeholder="端口"
                inputMode="numeric"
                autoComplete="off"
              />
            </label>
          </div>
          <label className={styles.connectionLabel}>
            6 位配对码或长令牌
            <input
              className={styles.connectionInput}
              value={credential}
              onInput={(event) => setCredential((event.target as HTMLInputElement).value)}
              placeholder="123456 或桌面端提供的令牌"
              autoComplete="off"
              autoCapitalize="off"
              spellcheck={false}
            />
          </label>
          <button className={styles.connectionSubmit} type="submit">连接</button>
        </form>

        <div className={styles.connectionRecovery}>
          {hasConnectionCredential(target) && issue && (
            issue.retryable || issue.kind === 'paused'
          ) && (
            <button className={styles.connectionAction} type="button" onClick={onRetry}>
              使用当前目标重试
            </button>
          )}
          <button className={styles.connectionAction} type="button" onClick={handleRescan}>
            重新扫码
          </button>
        </div>
      </section>
    </dialog>
  )
}

export function App() {
  const nicknameRef = useRef(loadNickname())
  const [nickname, setNickname] = useState(nicknameRef.current)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [historyOpen, setHistoryOpen] = useState(false)
  const [history, setHistory] = useState<HistoryItem[]>(loadHistory)
  const [autoSync, setAutoSync] = useState(false)
  const [keepAwake, setKeepAwake] = useState(loadKeepAwake)
  const {
    status,
    connectedDevice,
    errorMessage,
    connectionIssue,
    connectionTarget,
    needsConnectionInput,
    pendingCount,
    connectionEpoch,
    sendCommand,
    sendHello,
    connectWithTarget,
    retryConnection,
    editConnection,
  } = useWebSocket(nicknameRef)
  const [connectionEditorOpen, setConnectionEditorOpen] = useState(false)
  const isConnected = status === 'connected'
  const autoSyncSupported = supportsGraphemeDiff()
  const {
    draftText: text,
    syncState,
    syncError,
    pendingKind,
    resyncReason,
    failedKind,
    isBusy,
    updateDraft,
    setComposing,
    sendManual,
    clearDraft,
    sendEnter,
    sendDiscreteText,
    sendShortcut,
    retrySync,
    resumeSync,
    resolveUncertain,
  } = useInputSync({
    autoSync: autoSync && autoSyncSupported,
    isReady: isConnected,
    connectionEpoch,
    sendCommand,
  })
  const wakeLockSupported = typeof navigator !== 'undefined' && 'wakeLock' in navigator
  const wakeLockRef = useRef<WakeLockSentinel | null>(null)

  const openConnectionEditor = useCallback((clearCredentials = false) => {
    if (clearCredentials) editConnection(true)
    setConnectionEditorOpen(true)
  }, [editConnection])

  const handleConnectTarget = useCallback((target: ConnectionTarget) => {
    const connected = connectWithTarget(target)
    if (connected) setConnectionEditorOpen(false)
    return connected
  }, [connectWithTarget])

  const handleRetryConnection = useCallback(() => {
    const retried = retryConnection()
    if (retried) setConnectionEditorOpen(false)
    return retried
  }, [retryConnection])

  const connectionPanelOpen = needsConnectionInput || connectionEditorOpen

  useEffect(() => {
    try {
      localStorage.setItem(NICKNAME_KEY, nickname)
    } catch { /* ignore */ }
  }, [nickname])

  useEffect(() => {
    try {
      localStorage.setItem(HISTORY_KEY, JSON.stringify(history))
    } catch { /* ignore */ }
  }, [history])

  const addHistory = useCallback((value: string) => {
    if (!value) return
    const item = { id: genHistoryId(), text: value, createdAt: Date.now() }
    setHistory((prev) => [item, ...prev].slice(0, HISTORY_LIMIT))
  }, [])

  const sendTrackedText = useCallback((value: string) => {
    void sendDiscreteText(value).then((acknowledged) => {
      if (acknowledged) addHistory(value)
    })
  }, [addHistory, sendDiscreteText])

  const requestWakeLock = useCallback(async () => {
    if (!('wakeLock' in navigator) || document.visibilityState !== 'visible') return false
    try {
      wakeLockRef.current = await (navigator as unknown as WakeLockNavigator).wakeLock.request('screen')
      wakeLockRef.current.addEventListener('release', () => { wakeLockRef.current = null })
      return true
    } catch {
      return false
    }
  }, [])

  const releaseWakeLock = useCallback(async () => {
    const wakeLock = wakeLockRef.current
    wakeLockRef.current = null
    if (wakeLock) await wakeLock.release().catch(() => {})
  }, [])

  useEffect(() => {
    try {
      localStorage.setItem(KEEP_AWAKE_KEY, keepAwake ? '1' : '0')
    } catch { /* ignore */ }

    if (keepAwake) {
      requestWakeLock()
    } else {
      releaseWakeLock()
    }
  }, [keepAwake, releaseWakeLock, requestWakeLock])

  useEffect(() => {
    const handleVisibility = () => {
      if (keepAwake && document.visibilityState === 'visible') requestWakeLock()
    }
    document.addEventListener('visibilitychange', handleVisibility)
    return () => {
      document.removeEventListener('visibilitychange', handleVisibility)
      releaseWakeLock()
    }
  }, [keepAwake, releaseWakeLock, requestWakeLock])

  const handleNickname = useCallback((e: JSX.TargetedEvent<HTMLInputElement>) => {
    const v = (e.target as HTMLInputElement).value
    nicknameRef.current = v
    setNickname(v)
    sendHello()
  }, [sendHello])

  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const hasText = text.length > 0
  const commandBusy = isBusy || pendingCount > 0
  const canSendDiscrete = isConnected && !commandBusy && syncState !== 'uncertain'
  const canEnter = canSendDiscrete && (!autoSync || syncState === 'synced')
  const canClear = syncState !== 'uncertain' && (
    !autoSync || (
      isConnected
      && !commandBusy
      && syncState !== 'needs-resync'
    )
  )

  const handleInput = useCallback((e: JSX.TargetedEvent<HTMLTextAreaElement>) => {
    const value = (e.target as HTMLTextAreaElement).value
    updateDraft(value)
  }, [updateDraft])

  const handleCompositionStart = useCallback(() => {
    setComposing(true)
  }, [setComposing])

  const handleCompositionEnd = useCallback((e: JSX.TargetedEvent<HTMLTextAreaElement>) => {
    setComposing(false)
    const value = (e.target as HTMLTextAreaElement).value
    updateDraft(value)
  }, [setComposing, updateDraft])

  const handleClear = useCallback(() => {
    void clearDraft()
    textareaRef.current?.focus()
  }, [clearDraft])

  const handleSend = useCallback(() => {
    void sendManual().then((confirmedText) => {
      if (confirmedText !== null) addHistory(confirmedText)
    })
    textareaRef.current?.focus()
  }, [addHistory, sendManual])

  const handleEnter = useCallback(() => {
    void sendEnter().then((confirmedText) => {
      if (confirmedText) addHistory(confirmedText)
    })
    textareaRef.current?.focus()
  }, [addHistory, sendEnter])

  const handleShortcut = useCallback((modifiers: string[], key: string) => {
    void sendShortcut(modifiers, key)
  }, [sendShortcut])

  const handleResolveUncertain = useCallback((applied: boolean) => {
    const confirmedText = resolveUncertain(applied)
    if (confirmedText) addHistory(confirmedText)
  }, [addHistory, resolveUncertain])

  let statusText = '未连接'
  let deviceText: string | JSX.Element = '扫码或输入地址连接'
  if (status === 'needs-input') {
    statusText = '需要连接'
    deviceText = '请输入桌面端地址和配对信息'
  } else if (status === 'connecting' || status === 'reconnecting') {
    statusText = '连接中...'
    deviceText = '连接中...'
  } else if (status === 'connected') {
    statusText = '已连接'
    deviceText = connectedDevice
      ? <>已连接至 <span className={styles.deviceName}>{connectedDevice}</span></>
      : '已连接'
  } else if (status === 'pending') {
    statusText = '等待批准'
    deviceText = '请在桌面端批准此设备'
  } else if (status === 'paused') {
    statusText = '接收暂停'
    deviceText = errorMessage || '桌面端已暂停接收输入'
  } else if (status === 'pairing-invalid') {
    statusText = '配对无效'
    deviceText = errorMessage || '请重新扫码或输入新的配对信息'
  } else if (status === 'rejected') {
    statusText = '连接被拒绝'
    deviceText = errorMessage || '请在桌面端批准此设备，或重新配对'
  } else if (status === 'blacklisted') {
    statusText = '连接被拒绝'
    deviceText = errorMessage || '此设备已被拉黑，无法连接'
  } else if (status === 'disconnected') {
    if (connectionIssue?.kind === 'protocol-error') {
      statusText = '协议错误'
      deviceText = connectionIssue.message
    } else if (connectionIssue?.kind === 'network-error') {
      statusText = '网络错误'
      deviceText = connectionIssue.message
    } else if (connectionIssue?.kind === 'server-error') {
      statusText = '桌面端错误'
      deviceText = connectionIssue.message
    } else {
      statusText = '桌面离线'
      deviceText = connectionIssue?.message || '连接已断开，正在重连...'
    }
  }

  let syncText = autoSyncSupported
    ? (autoSync ? '等待输入' : '草稿仅保存在手机')
    : '当前浏览器不支持可靠自动发送，请使用手动发送'
  if (syncState === 'queued') syncText = '等待发送'
  else if (syncState === 'sending') {
    if (pendingKind === 'clear') syncText = '正在清空'
    else if (pendingKind === 'enter') syncText = '正在发送回车'
    else syncText = '正在等待桌面确认'
  } else if (syncState === 'synced') syncText = '已同步'
  else if (syncState === 'failed') syncText = '发送失败，草稿已保留'
  else if (syncState === 'uncertain') syncText = '未收到确认，请核对桌面结果'
  else if (syncState === 'needs-resync') {
    syncText = resyncReason === 'discrete' ? '当前光标上下文需要重新同步' : '重连后需要确认同步'
  }

  const canRetryConnection = Boolean(
    connectionTarget
    && status !== 'connected'
    && status !== 'needs-input'
    && (connectionIssue?.kind === 'offline'
      || connectionIssue?.kind === 'network-error'
      || connectionIssue?.kind === 'paused'),
  )
  const needsFreshPairing = status === 'paused'
    || status === 'pairing-invalid'
    || status === 'blacklisted'

  return (
    <div className={styles.body}>
      {errorMessage && !connectionPanelOpen && (
        <div className={styles.toastWrap}>
          <div className={styles.toast} role="alert" aria-live="assertive">
            {errorMessage}
          </div>
        </div>
      )}
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <h1 className={styles.title}>LanType</h1>
          <button
            className={styles.btnSettings}
            onClick={() => { setSettingsOpen(true); setHistoryOpen(false) }}
            aria-label="设置"
          >
            <IconGear size={20} />
          </button>
        </div>
        <div className={styles.headerRight}>
          <button
            className={cx(styles.toggle, autoSync && styles.toggleActive)}
            onClick={() => setAutoSync((enabled) => !enabled)}
            disabled={!autoSyncSupported}
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
              status === 'needs-input' && styles.statusReconnecting,
              status === 'pending' && styles.statusReconnecting,
              status === 'paused' && styles.statusDisconnected,
              (status === 'pairing-invalid' || status === 'rejected') && styles.statusDisconnected,
            )}
            role="status"
            aria-live="polite"
          >
            <span
              className={cx(
                styles.dot,
                status === 'connected' && styles.dotConnected,
                status === 'disconnected' && styles.dotDisconnected,
                (status === 'reconnecting' || status === 'connecting') && styles.dotReconnecting,
                status === 'needs-input' && styles.dotReconnecting,
                status === 'pending' && styles.dotReconnecting,
                status === 'paused' && styles.dotDisconnected,
                (status === 'pairing-invalid' || status === 'rejected') && styles.dotDisconnected,
                status === 'blacklisted' && styles.dotDisconnected,
              )}
            />
            <span>{statusText}</span>
          </div>
        </div>
      </div>
      <div className={styles.deviceRow}>
        <div>{deviceText}</div>
        <div className={styles.connectionActions}>
          {canRetryConnection && (
            <button className={styles.connectionAction} type="button" onClick={handleRetryConnection}>
              重试连接
            </button>
          )}
          {!needsConnectionInput && (
            <button className={styles.connectionAction} type="button" onClick={() => openConnectionEditor(false)}>
              修改连接
            </button>
          )}
          {needsFreshPairing && (
            <button className={styles.connectionAction} type="button" onClick={() => openConnectionEditor(true)}>
              重新扫码
            </button>
          )}
        </div>
      </div>
      <div
        className={cx(
          styles.syncRow,
          syncState === 'synced' && styles.syncGood,
          (syncState === 'queued' || syncState === 'sending') && styles.syncWorking,
          (syncState === 'failed' || syncState === 'uncertain' || syncState === 'needs-resync') && styles.syncProblem,
        )}
      >
        <span
          id="sync-status"
          className={styles.syncText}
          role="status"
          aria-live="polite"
          aria-atomic="true"
        >
          {syncError || syncText}
        </span>
        {syncState === 'failed' && failedKind === 'auto' && isConnected && (
          <button className={styles.syncAction} onClick={retrySync} type="button">重试同步</button>
        )}
        {syncState === 'needs-resync' && isConnected && (
          <button className={styles.syncAction} onClick={resumeSync} type="button">
            {resyncReason === 'discrete' ? '从当前光标同步' : '继续同步'}
          </button>
        )}
        {syncState === 'uncertain' && isConnected && (
          <div className={styles.syncActions}>
            <button className={styles.syncAction} onClick={() => handleResolveUncertain(true)} type="button">已执行</button>
            <button className={styles.syncAction} onClick={() => handleResolveUncertain(false)} type="button">未执行</button>
          </div>
        )}
      </div>
      <div className={styles.inputWrap}>
        <textarea
          ref={textareaRef}
          className={styles.textarea}
          value={text}
          onInput={handleInput}
          onCompositionStart={handleCompositionStart}
          onCompositionEnd={handleCompositionEnd}
          placeholder="在此输入文字..."
          aria-describedby="sync-status"
          aria-busy={pendingKind !== null}
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
          disabled={!canEnter}
          aria-label="回车"
        >
          <IconEnter size={22} />
        </button>
        {(!autoSync || !hasText) && (
          <button
            className={cx(styles.btn, styles.btnSend, autoSync && styles.hidden)}
            onClick={handleSend}
            disabled={!canSendDiscrete || !hasText}
            aria-label="发送"
            aria-busy={pendingKind === 'manual'}
          >
            <IconSend size={22} />
          </button>
        )}
        {hasText && (
          <button
            className={cx(styles.btn, styles.btnClear)}
            onClick={handleClear}
            disabled={!canClear}
            aria-label="清空"
          >
            <IconClear size={20} />
          </button>
        )}
      </div>
      <div className={styles.footer}>文字同步到桌面端</div>

      <ConnectionPanel
        open={connectionPanelOpen}
        target={connectionTarget}
        issue={connectionIssue}
        onConnect={handleConnectTarget}
        onRetry={handleRetryConnection}
        onEdit={openConnectionEditor}
        onClose={() => setConnectionEditorOpen(false)}
      />

      <ShortcutPanel sendKeys={handleShortcut} isConnected={canSendDiscrete} />
      <PhrasePanel sendType={sendTrackedText} isConnected={canSendDiscrete} />

      <HistoryPanel
        open={historyOpen}
        history={history}
        isConnected={canSendDiscrete}
        onOpen={() => { setHistoryOpen(true); setSettingsOpen(false) }}
        onSend={sendTrackedText}
        onClose={() => setHistoryOpen(false)}
      />

      <SettingsPanel
        open={settingsOpen}
        nickname={nickname}
        keepAwake={keepAwake}
        wakeLockSupported={wakeLockSupported}
        onNickname={handleNickname}
        onKeepAwake={setKeepAwake}
        onClose={() => setSettingsOpen(false)}
      />
    </div>
  )
}
