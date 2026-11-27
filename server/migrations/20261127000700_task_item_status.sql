-- T3.0.3: a task's item carries its status so filters can match it (F1, G3). From now on the
-- tasks module keeps it in sync; this fills it for the tasks that already exist.
UPDATE items i
SET status = CASE
        WHEN EXISTS (SELECT 1 FROM subtasks s WHERE s.task_id = i.id AND s.done_at IS NULL)
            THEN 'open'
        ELSE 'done'
    END
WHERE i.kind = 'task';
