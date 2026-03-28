import { useAppStore } from '../../store/appStore';
import { ServerStrip } from './ServerStrip';
import { ChannelSidebar } from './ChannelSidebar';
import { ChatArea } from './ChatArea';
import { MemberList } from './MemberList';
import { SettingsPanel } from '../settings/SettingsPanel';
import styles from './MainLayout.module.css';

export function MainLayout() {
  const memberListOpen = useAppStore((s) => s.ui.memberListOpen);
  const settingsOpen = useAppStore((s) => s.ui.settingsOpen);

  return (
    <div className={styles.layout}>
      <ServerStrip />
      <ChannelSidebar />
      <ChatArea />
      {memberListOpen && <MemberList />}
      {settingsOpen && <SettingsPanel />}
    </div>
  );
}
