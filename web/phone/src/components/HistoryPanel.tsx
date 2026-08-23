import { IconHistory, IconSend } from './icons'
import styles from './HistoryPanel.module.css'

function cx(...names: (string | false | null | undefined)[]): string {
  return names.filter(Boolean).join(' ')
}

export interface HistoryItem {
  id: string
  text: string
  createdAt: number
}

interface HistoryPanelProps {
  open: boolean
  history: HistoryItem[]
  isConnected: boolean
  onOpen: () => void
  onSend: (text: string) => void
  onClose: () => void
}

function formatTime(value: number): string {
  return new Date(value).toLocaleString(undefined, {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function HistoryPanel({ open, history, isConnected, onOpen, onSend, onClose }: HistoryPanelProps) {
  return (
    <>
      <button
        className={cx(styles.trigger, open && styles.triggerActive)}
        onClick={() => { if (open) onClose(); else onOpen() }}
        aria-label="发送历史"
        type="button"
      >
        <IconHistory size={20} />
      </button>

      <div
        className={cx(styles.overlay, open && styles.overlayOpen)}
        onClick={(e) => { if (e.target === e.currentTarget) onClose() }}
      >
        <div className={styles.panel}>
          <div className={styles.head}>
            <h2 className={styles.title}>发送历史</h2>
            <span className={styles.count}>{history.length}/50</span>
          </div>

          {history.length > 0 ? (
            <div className={styles.list}>
              {history.map((item) => (
                <button
                  key={item.id}
                  className={styles.item}
                  onClick={() => { onSend(item.text); onClose() }}
                  disabled={!isConnected}
                  type="button"
                >
                  <span className={styles.itemText}>{item.text}</span>
                  <span className={styles.itemMeta}>
                    <span>{formatTime(item.createdAt)}</span>
                    <IconSend size={14} />
                  </span>
                </button>
              ))}
            </div>
          ) : (
            <div className={styles.empty}>暂无发送历史</div>
          )}

          <button className={styles.close} onClick={onClose} type="button">关闭</button>
        </div>
      </div>
    </>
  )
}
