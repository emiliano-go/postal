class CallHistoryState {
  revision = $state(0);

  refresh() { this.revision++; }
}

export const callHistory = new CallHistoryState();
