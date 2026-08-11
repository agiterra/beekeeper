import type { ChannelSortMode } from "@/features/sidebar/lib/channelSortPreference";
import {
  SectionActionsMenu,
  SectionQuickAction,
} from "@/features/sidebar/ui/CustomChannelSection";

/** The Direct-messages header actions (new message + section menu),
 * extracted verbatim from AppSidebar. */
export function DmSectionActions({
  onNewMessage,
  onOpenChange,
  sortMode,
  onSortModeChange,
}: {
  onNewMessage: () => void;
  onOpenChange: (open: boolean) => void;
  sortMode: ChannelSortMode;
  onSortModeChange: (mode: ChannelSortMode) => void;
}) {
  return (
    <div className="absolute right-1 top-1/2 z-10 flex -translate-y-1/2 items-center gap-0.5">
      <SectionQuickAction
        label="New message"
        onClick={onNewMessage}
        testId="section-actions-dms-quick-create"
      />
      <SectionActionsMenu
        sectionLabel="direct messages"
        testId="section-actions-dms"
        onOpenChange={onOpenChange}
        onNewMessage={onNewMessage}
        sortMode={sortMode}
        onSortModeChange={onSortModeChange}
      />
    </div>
  );
}
