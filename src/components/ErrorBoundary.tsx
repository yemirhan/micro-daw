import { Component, type ErrorInfo, type ReactNode } from 'react';
import { AlertTriangle } from 'lucide-react';
import { Button } from '@/components/ui/button';

interface Props {
  children: ReactNode;
  zoneName: string;
  fallbackVariant: 'panel' | 'inline';
  onReset?: () => void;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { hasError: false, error: null };

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(`[ErrorBoundary:${this.props.zoneName}]`, error, info.componentStack);
  }

  handleReset = () => {
    this.setState({ hasError: false, error: null });
    this.props.onReset?.();
  };

  render() {
    if (!this.state.hasError) return this.props.children;

    const message = this.state.error?.message || 'Unknown error';

    if (this.props.fallbackVariant === 'inline') {
      return (
        <div className="flex items-center gap-2 border-b border-border bg-card/60 px-4 py-2 text-sm text-muted-foreground">
          <AlertTriangle className="size-4 shrink-0 text-destructive" />
          <span className="truncate font-mono text-xs">
            {this.props.zoneName} encountered an error: {message}
          </span>
          <Button variant="ghost" size="xs" onClick={this.handleReset} className="ml-auto shrink-0">
            Retry
          </Button>
        </div>
      );
    }

    return (
      <div className="flex flex-1 items-center justify-center p-8">
        <div className="flex max-w-md flex-col items-center gap-4 rounded-lg border border-border bg-card/60 p-8 text-center backdrop-blur-sm">
          <AlertTriangle className="size-8 text-destructive" />
          <div>
            <p className="text-sm font-medium text-foreground">
              {this.props.zoneName} encountered an error
            </p>
            <p className="mt-1 font-mono text-xs text-muted-foreground">{message}</p>
          </div>
          <Button variant="outline" size="sm" onClick={this.handleReset}>
            Try Again
          </Button>
        </div>
      </div>
    );
  }
}
