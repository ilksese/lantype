import { useCallback, useEffect, useRef, useState } from 'preact/compat'
import { getTextDiff } from '../textDiff'
import {
  isSendFailure,
  type CommandAck,
  type InputCommand,
} from './useWebSocket'

export type SyncState =
  | 'idle'
  | 'queued'
  | 'sending'
  | 'synced'
  | 'failed'
  | 'uncertain'
  | 'needs-resync'

export type PendingKind = 'auto' | 'manual' | 'clear' | 'enter' | 'text' | 'keys' | null
export type ResyncReason = 'connection' | 'discrete' | null

interface UseInputSyncOptions {
  autoSync: boolean
  isReady: boolean
  connectionEpoch: number
  sendCommand: (command: InputCommand) => Promise<CommandAck>
}

interface RecoveryBase {
  base: string
  wasBaselineValid: boolean
  previousReason: ResyncReason
}

type UncertainOperation =
  | (RecoveryBase & { kind: 'auto'; target: string })
  | (RecoveryBase & { kind: 'manual'; text: string; revision: number })
  | (RecoveryBase & { kind: 'reset'; revision: number })
  | (RecoveryBase & { kind: 'discrete' })

const DEBOUNCE_MS = 40

function errorText(value: unknown): string {
  return value instanceof Error ? value.message : '发送失败，草稿已保留'
}

export function useInputSync({
  autoSync,
  isReady,
  connectionEpoch,
  sendCommand,
}: UseInputSyncOptions) {
  const [draftText, setDraftText] = useState('')
  const [syncState, setSyncState] = useState<SyncState>('idle')
  const [syncError, setSyncError] = useState<string | null>(null)
  const [pendingKind, setPendingKind] = useState<PendingKind>(null)
  const [resyncReason, setResyncReason] = useState<ResyncReason>(null)
  const [failedKind, setFailedKind] = useState<PendingKind>(null)
  const draftRef = useRef('')
  const revisionRef = useRef(0)
  const baselineRef = useRef('')
  const baselineValidRef = useRef(true)
  const syncStateRef = useRef<SyncState>('idle')
  const pendingRef = useRef<object | null>(null)
  const uncertainRef = useRef<UncertainOperation | null>(null)
  const resyncReasonRef = useRef<ResyncReason>(null)
  const failedKindRef = useRef<PendingKind>(null)
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const flushAutoRef = useRef<() => void>(() => {})
  const autoRef = useRef(autoSync)
  const readyRef = useRef(isReady)
  const composingRef = useRef(false)
  const everReadyRef = useRef(false)
  const epochRef = useRef(0)

  autoRef.current = autoSync
  readyRef.current = isReady

  const transition = useCallback((next: SyncState) => {
    syncStateRef.current = next
    setSyncState(next)
  }, [])

  const setFailureKind = useCallback((kind: PendingKind) => {
    failedKindRef.current = kind
    setFailedKind(kind)
  }, [])

  const setReason = useCallback((reason: ResyncReason) => {
    resyncReasonRef.current = reason
    setResyncReason(reason)
  }, [])

  const clearDebounce = useCallback(() => {
    if (!debounceRef.current) return
    clearTimeout(debounceRef.current)
    debounceRef.current = null
  }, [])

  const replaceDraft = useCallback((value: string) => {
    revisionRef.current += 1
    draftRef.current = value
    setDraftText(value)
  }, [])

  const requireResync = useCallback((reason: Exclude<ResyncReason, null>, invalidate: boolean) => {
    clearDebounce()
    if (invalidate) baselineValidRef.current = false
    setReason(reason)
    setFailureKind(null)
    transition('needs-resync')
  }, [clearDebounce, setFailureKind, setReason, transition])

  const execute = useCallback(async (
    command: InputCommand,
    kind: Exclude<PendingKind, null>,
    recovery: UncertainOperation,
  ): Promise<boolean> => {
    if (pendingRef.current) {
      setSyncError('上一条命令仍在等待确认')
      return false
    }

    clearDebounce()
    const token = {}
    pendingRef.current = token
    setPendingKind(kind)
    setFailureKind(null)
    setSyncError(null)
    transition('sending')

    try {
      await sendCommand(command)
      setSyncError(null)
      return true
    } catch (error) {
      setSyncError(errorText(error))
      if (isSendFailure(error) && error.uncertain) {
        uncertainRef.current = recovery
        transition('uncertain')
      } else if (isSendFailure(error) && error.code === 'not_connected') {
        setReason('connection')
        transition('needs-resync')
      } else {
        setFailureKind(kind)
        transition('failed')
      }
      return false
    } finally {
      if (pendingRef.current === token) {
        pendingRef.current = null
        setPendingKind(null)
      }
    }
  }, [clearDebounce, sendCommand, setFailureKind, setReason, transition])

  const scheduleAuto = useCallback((force = false) => {
    clearDebounce()
    if (
      !autoRef.current
      || !readyRef.current
      || composingRef.current
      || pendingRef.current
      || !baselineValidRef.current
    ) return
    if (!force && (
      syncStateRef.current === 'failed'
      || syncStateRef.current === 'uncertain'
      || syncStateRef.current === 'needs-resync'
    )) return

    if (draftRef.current === baselineRef.current) {
      setFailureKind(null)
      setSyncError(null)
      transition('synced')
      return
    }

    transition('queued')
    debounceRef.current = setTimeout(() => {
      debounceRef.current = null
      flushAutoRef.current()
    }, DEBOUNCE_MS)
  }, [clearDebounce, setFailureKind, transition])

  const flushAuto = useCallback(async () => {
    if (
      !autoRef.current
      || !readyRef.current
      || composingRef.current
      || pendingRef.current
      || !baselineValidRef.current
    ) return
    const base = baselineRef.current
    const target = draftRef.current
    const diff = getTextDiff(base, target)
    if (diff.backspace === 0 && !diff.text) {
      transition('synced')
      return
    }

    const recovery: UncertainOperation = {
      kind: 'auto',
      base,
      target,
      wasBaselineValid: true,
      previousReason: null,
    }
    const acknowledged = await execute({ type: 'diff', ...diff }, 'auto', recovery)
    if (!acknowledged) return

    baselineRef.current = target
    baselineValidRef.current = true
    setReason(null)
    if (!autoRef.current) {
      transition('idle')
    } else if (draftRef.current !== target) {
      scheduleAuto(true)
    } else {
      transition('synced')
    }
  }, [execute, scheduleAuto, setReason, transition])

  flushAutoRef.current = () => { void flushAuto() }

  const updateDraft = useCallback((value: string, synchronize = true) => {
    replaceDraft(value)
    if (!synchronize || composingRef.current) {
      clearDebounce()
      return
    }
    if (!autoRef.current) {
      if (syncStateRef.current === 'failed') {
        setFailureKind(null)
        setSyncError(null)
        transition('idle')
      }
      return
    }
    if (pendingRef.current) return
    if (!readyRef.current) {
      if (everReadyRef.current) requireResync('connection', false)
      return
    }
    scheduleAuto()
  }, [clearDebounce, replaceDraft, requireResync, scheduleAuto, setFailureKind, transition])

  const setComposing = useCallback((composing: boolean) => {
    composingRef.current = composing
    if (composing) clearDebounce()
  }, [clearDebounce])

  const sendManual = useCallback(async (): Promise<string | null> => {
    const value = draftRef.current
    if (!value || autoRef.current || !readyRef.current || composingRef.current) return null
    const revision = revisionRef.current
    const recovery: UncertainOperation = {
      kind: 'manual',
      text: value,
      revision,
      base: baselineRef.current,
      wasBaselineValid: baselineValidRef.current,
      previousReason: resyncReasonRef.current,
    }
    const acknowledged = await execute({ type: 'type', text: value }, 'manual', recovery)
    if (!acknowledged) return null

    baselineRef.current = ''
    baselineValidRef.current = true
    if (revisionRef.current === revision) {
      replaceDraft('')
      setReason(null)
      transition(autoRef.current ? 'synced' : 'idle')
    } else {
      requireResync('discrete', true)
    }
    return value
  }, [execute, replaceDraft, requireResync, setReason, transition])

  const clearDraft = useCallback(async (): Promise<boolean> => {
    const currentState = syncStateRef.current
    if (pendingRef.current || currentState === 'uncertain') return false
    if (!autoRef.current) {
      clearDebounce()
      replaceDraft('')
      setFailureKind(null)
      setSyncError(null)
      transition('idle')
      return true
    }
    if (!readyRef.current || !baselineValidRef.current) return false

    clearDebounce()
    const base = baselineRef.current
    if (!base) {
      replaceDraft('')
      setReason(null)
      setSyncError(null)
      transition('synced')
      return true
    }

    const revision = revisionRef.current
    const diff = getTextDiff(base, '')
    const recovery: UncertainOperation = {
      kind: 'reset',
      base,
      revision,
      wasBaselineValid: true,
      previousReason: resyncReasonRef.current,
    }
    const acknowledged = await execute({ type: 'diff', ...diff }, 'clear', recovery)
    if (!acknowledged) return false

    baselineRef.current = ''
    baselineValidRef.current = true
    if (revisionRef.current === revision) {
      replaceDraft('')
      setReason(null)
      transition('synced')
    } else {
      requireResync('discrete', true)
    }
    return true
  }, [clearDebounce, execute, replaceDraft, requireResync, setFailureKind, setReason, transition])

  const sendEnter = useCallback(async (): Promise<string | null> => {
    if (!readyRef.current) return null
    const value = draftRef.current
    if (autoRef.current) {
      if (
        !baselineValidRef.current
        || syncStateRef.current !== 'synced'
        || baselineRef.current !== value
      ) {
        setSyncError('请等待当前文字同步完成')
        return null
      }

      const revision = revisionRef.current
      const recovery: UncertainOperation = {
        kind: 'reset',
        base: baselineRef.current,
        revision,
        wasBaselineValid: true,
        previousReason: null,
      }
      const acknowledged = await execute({ type: 'type', text: '\n' }, 'enter', recovery)
      if (!acknowledged) return null

      baselineRef.current = ''
      baselineValidRef.current = true
      if (revisionRef.current === revision) {
        replaceDraft('')
        setReason(null)
        transition('synced')
      } else {
        requireResync('discrete', true)
      }
      return value
    }

    const recovery: UncertainOperation = {
      kind: 'discrete',
      base: baselineRef.current,
      wasBaselineValid: baselineValidRef.current,
      previousReason: resyncReasonRef.current,
    }
    const acknowledged = await execute({ type: 'type', text: '\n' }, 'enter', recovery)
    if (!acknowledged) return null
    requireResync('discrete', true)
    return ''
  }, [execute, replaceDraft, requireResync, setReason, transition])

  const sendDiscreteText = useCallback(async (value: string): Promise<boolean> => {
    if (!value || !readyRef.current) return false
    const recovery: UncertainOperation = {
      kind: 'discrete',
      base: baselineRef.current,
      wasBaselineValid: baselineValidRef.current,
      previousReason: resyncReasonRef.current,
    }
    const acknowledged = await execute({ type: 'type', text: value }, 'text', recovery)
    if (!acknowledged) return false
    requireResync('discrete', true)
    return true
  }, [execute, requireResync])

  const sendShortcut = useCallback(async (modifiers: string[], key: string): Promise<boolean> => {
    if (!readyRef.current) return false
    const recovery: UncertainOperation = {
      kind: 'discrete',
      base: baselineRef.current,
      wasBaselineValid: baselineValidRef.current,
      previousReason: resyncReasonRef.current,
    }
    const acknowledged = await execute({ type: 'keys', modifiers, key }, 'keys', recovery)
    if (!acknowledged) return false
    requireResync('discrete', true)
    return true
  }, [execute, requireResync])

  const retrySync = useCallback(() => {
    if (
      failedKindRef.current !== 'auto'
      || !autoRef.current
      || !readyRef.current
      || pendingRef.current
      || !baselineValidRef.current
    ) return
    setFailureKind(null)
    setSyncError(null)
    transition('idle')
    scheduleAuto(true)
  }, [scheduleAuto, setFailureKind, transition])

  const resumeSync = useCallback(() => {
    if (!readyRef.current || pendingRef.current || syncStateRef.current !== 'needs-resync') return
    if (resyncReasonRef.current === 'discrete') baselineRef.current = ''
    baselineValidRef.current = true
    setReason(null)
    setSyncError(null)
    if (autoRef.current) scheduleAuto(true)
    else transition('idle')
  }, [scheduleAuto, setReason, transition])

  const resolveUncertain = useCallback((applied: boolean): string | null => {
    const recovery = uncertainRef.current
    if (!recovery || pendingRef.current) return null
    uncertainRef.current = null
    setSyncError(null)
    let confirmedText: string | null = null

    if (applied) {
      if (recovery.kind === 'auto') {
        baselineRef.current = recovery.target
        baselineValidRef.current = true
      } else if (recovery.kind === 'manual') {
        confirmedText = recovery.text
        baselineRef.current = ''
        baselineValidRef.current = true
        if (revisionRef.current === recovery.revision) replaceDraft('')
        else {
          requireResync('discrete', true)
          return confirmedText
        }
      } else if (recovery.kind === 'reset') {
        baselineRef.current = ''
        baselineValidRef.current = true
        if (revisionRef.current === recovery.revision) replaceDraft('')
        else {
          requireResync('discrete', true)
          return null
        }
      } else {
        requireResync('discrete', true)
        return null
      }
    } else {
      baselineRef.current = recovery.base
      baselineValidRef.current = recovery.wasBaselineValid
      if (!recovery.wasBaselineValid) {
        requireResync(recovery.previousReason || 'discrete', true)
        return null
      }
    }

    setReason(null)
    if (autoRef.current && readyRef.current) scheduleAuto(true)
    else transition('idle')
    return confirmedText
  }, [replaceDraft, requireResync, scheduleAuto, setReason, transition])

  useEffect(() => {
    clearDebounce()
    if (!autoSync) {
      if (syncStateRef.current === 'queued') transition('idle')
      return
    }
    if (!isReady || pendingRef.current) return
    if (!baselineValidRef.current) {
      requireResync('discrete', true)
      return
    }
    if (syncStateRef.current !== 'uncertain' && syncStateRef.current !== 'needs-resync') {
      setFailureKind(null)
      setSyncError(null)
      scheduleAuto(true)
    }
  }, [autoSync, clearDebounce, isReady, requireResync, scheduleAuto, setFailureKind, transition])

  useEffect(() => {
    if (!isReady) {
      clearDebounce()
      if (
        everReadyRef.current
        && !pendingRef.current
        && syncStateRef.current !== 'uncertain'
        && !(syncStateRef.current === 'needs-resync' && resyncReasonRef.current === 'discrete')
      ) requireResync('connection', false)
      return
    }

    if (!everReadyRef.current) {
      everReadyRef.current = true
      epochRef.current = connectionEpoch
      if (autoRef.current && baselineValidRef.current) scheduleAuto(true)
      return
    }

    if (connectionEpoch !== epochRef.current) {
      epochRef.current = connectionEpoch
      if (
        syncStateRef.current !== 'uncertain'
        && !(syncStateRef.current === 'needs-resync' && resyncReasonRef.current === 'discrete')
      ) requireResync('connection', false)
    }
  }, [clearDebounce, connectionEpoch, isReady, requireResync, scheduleAuto])

  useEffect(() => () => clearDebounce(), [clearDebounce])

  return {
    draftText,
    syncState,
    syncError,
    pendingKind,
    resyncReason,
    failedKind,
    isBusy: pendingKind !== null || syncState === 'queued',
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
  }
}
