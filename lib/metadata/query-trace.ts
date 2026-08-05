import InetAddress = require("../types/inet-address");
import Uuid = require("../types/uuid");

/**
 * A single event that happened during a traced query execution.
 * @alias module:metadata~TracingEvent
 */
class TracingEvent {
    id: Uuid;
    activity: string | null;
    source: InetAddress | null;
    elapsed: number | null;
    thread: string | null;
}

/**
 * Tracing information retrieved for a query that was executed with tracing enabled.
 * @alias module:metadata~QueryTrace
 */
class QueryTrace {
    requestType: string | null;
    coordinator: InetAddress | null;
    parameters: { [key: string]: string };
    startedAt: bigint | null;
    duration: number | null;
    clientAddress: InetAddress | null;
    events: TracingEvent[];
}

export { QueryTrace, TracingEvent };

