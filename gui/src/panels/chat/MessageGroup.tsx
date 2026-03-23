import { type Component, For, Show } from 'solid-js';
import type { AssistantMessage } from '../../stores/claudeStore';
import { ThinkingBlock } from './ThinkingBlock';
import { ToolCallCard } from './ToolCallCard';
import { StreamingText } from './StreamingText';
import styles from './MessageGroup.module.css';

export interface MessageGroupProps {
  message: AssistantMessage;
}

export const MessageGroup: Component<MessageGroupProps> = (props) => {
  return (
    <div data-testid="message-group" class={styles.group}>
      <Show when={props.message.thinkingText}>
        <ThinkingBlock
          text={props.message.thinkingText}
          complete={props.message.thinkingComplete}
        />
      </Show>
      <For each={props.message.toolCalls}>
        {(toolCall) => <ToolCallCard toolCall={toolCall} />}
      </For>
      <StreamingText
        text={props.message.responseText}
        streaming={!props.message.complete}
      />
    </div>
  );
};
