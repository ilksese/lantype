import type { JSX } from 'preact/compat'
import styles from './SettingsPanel.module.css'

function cx(...names: (string | false | null | undefined)[]): string {
  return names.filter(Boolean).join(' ')
}

interface SettingsPanelProps {
  open: boolean
  nickname: string
  onNickname: (e: JSX.TargetedEvent<HTMLInputElement>) => void
  onClose: () => void
}

export function SettingsPanel({ open, nickname, onNickname, onClose }: SettingsPanelProps) {
  return (
    <div
      className={cx(styles.overlay, open && styles.overlayOpen)}
      onClick={(e) => { if (e.target === e.currentTarget) onClose() }}
    >
      <div className={styles.settingsPanel}>
        <h2 className={styles.settingsTitle}>设置</h2>
        <div className={styles.settingsRow}>
          <span className={styles.settingsLabel}>昵称</span>
          <input
            className={styles.nicknameInput}
            value={nickname}
            onInput={onNickname}
            placeholder="显示在接收端设备列表"
            maxLength={20}
          />
        </div>
        <button
          className={styles.btnCloseSettings}
          onClick={onClose}
        >
          关闭
        </button>
      </div>
    </div>
  )
}
