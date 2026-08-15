import { useState, useEffect, useRef, useCallback } from 'preact/compat'

export interface ServerMessage {
  type: 'connected' | 'error' | 'pong'
  device?: string
  client_id?: string
  message?: string
}

export interface Shortcut {
  id: string
  modifiers: string[]
  key: string
}

export type ClientStatus = 'disconnected' | 'connecting' | 'connected' | 'reconnecting' | 'blacklisted'

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

export function useWebSocket(nicknameRef: { current: string }) {
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
      ws.send(JSON.stringify({ type: 'hello', device_name: nicknameRef.current || getFriendlyName() }))
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
      // A missing pong means the socket is dead. Force a full reconnect
      // rather than only ws.close(): the browser may not fire onclose for a
      // half-open connection, leaving the UI stuck on "已连接" while sends
      // silently drop.
      if (wsRef.current === ws) {
        connect(true)
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
      // Waking from a long screen-off leaves a possibly-stale socket that the
      // browser still reports as OPEN. Force a fresh connection (like a page
      // refresh) so typing is reliably delivered to the receiver.
      if (document.visibilityState === 'visible' && status !== 'blacklisted') {
        connect(true)
      }
    }
    const onPageShow = (e: PageTransitionEvent) => {
      // Restoring from bfcache (e.persisted) can leave the WebSocket stale on
      // any browser that supports the back/forward cache. Force a fresh
      // connection there. Initial page load (persisted=false) is already
      // handled by the connect effect, so only reconnect on bfcache restore.
      if (e.persisted) {
        if (status !== 'blacklisted') connect(true)
      } else {
        ensureConnected()
      }
    }

    document.addEventListener('visibilitychange', onVisible)
    window.addEventListener('pageshow', onPageShow)
    return () => {
      document.removeEventListener('visibilitychange', onVisible)
      window.removeEventListener('pageshow', onPageShow)
    }
  }, [connect, ensureConnected, status])

  const sendHello = useCallback(() => {
    sendMessage({ type: 'hello', device_name: nicknameRef.current || getFriendlyName() })
  }, [sendMessage, nicknameRef])

  const sendTypeAndReset = useCallback((text: string) => {
    sendMessage({ type: 'type', text })
    prevTextRef.current = ''
  }, [sendMessage])

  const resetPrev = useCallback(() => { prevTextRef.current = '' }, [])

  return { status, connectedDevice, errorMessage, sendDiff, sendKeys, sendHello, sendType: sendTypeAndReset, resetPrev }
}
