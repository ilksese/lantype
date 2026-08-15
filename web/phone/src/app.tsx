import { useState, useEffect, useRef, useCallback, type JSX } from 'preact/compat'
import type { ServerMessage, ClientStatus } from './types'
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
    const host = window.location.hostname
    const url = 'ws://' + host + ':' + wsPort

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

  return { status, connectedDevice, errorMessage, sendDiff, sendType: useCallback((text: string) => {
    sendMessage({ type: 'type', text })
    prevTextRef.current = ''
  }, [sendMessage]), resetPrev: useCallback(() => { prevTextRef.current = '' }, []) }
}

export function App() {
  const { status, connectedDevice, errorMessage, sendDiff, sendType, resetPrev } = useWebSocket()
  const [text, setText] = useState('')
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [autoSync, setAutoSync] = useState(false)
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const isComposing = useRef(false)

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
    statusText = '已被拉黑'
    deviceText = '此设备已被拉黑，无法连接'
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
