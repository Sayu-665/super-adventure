package dev.shaderbridge.render.raw;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;

import java.util.List;
import org.junit.jupiter.api.Test;

/** Barrier and layout planning of raw dispatches: own resources initialized once, per-frame clears, full barriers around. */
class CommandPlanTest {
    private static List<CommandPlan.Step<String>> steps(ResourceInit<String> init, List<String> used, long frame) {
        return CommandPlan.of(init.before(used, frame));
    }

    @Test
    void aDispatchWithoutOwnResourcesIsBracketedByBarriers() {
        assertEquals(List.of(new CommandPlan.Step.Barrier<>(List.of()), new CommandPlan.Step.Run<>(), new CommandPlan.Step.Barrier<>(List.of())),
            steps(new ResourceInit<>(), List.of("minecraft texture"), 1));
    }

    @Test
    void firstUseTransitionsAndClearsBeforeTheRun() {
        ResourceInit<String> init = new ResourceInit<>();
        init.created("lut", false);
        init.created("ssbo", false);
        assertEquals(List.of(
            new CommandPlan.Step.Barrier<>(List.of("lut", "ssbo")),
            new CommandPlan.Step.Fill<>("lut", true),
            new CommandPlan.Step.Fill<>("ssbo", true),
            new CommandPlan.Step.Barrier<>(List.of()),
            new CommandPlan.Step.Run<>(),
            new CommandPlan.Step.Barrier<>(List.of())), steps(init, List.of("lut", "ssbo", "lut"), 1));
        assertEquals(3, steps(init, List.of("lut", "ssbo"), 1).size(), "initialized once");
        assertEquals(3, steps(init, List.of("lut", "ssbo"), 2).size(), "they keep their contents across frames");
    }

    @Test
    void imagesClearedEveryFrameAreClearedOncePerFrame() {
        ResourceInit<String> init = new ResourceInit<>();
        init.created("debug", true);
        steps(init, List.of("debug"), 7);
        assertEquals(3, steps(init, List.of("debug"), 7).size(), "already cleared in frame 7");
        List<CommandPlan.Step<String>> next = steps(init, List.of("debug"), 8);
        assertEquals(List.of(), assertInstanceOf(CommandPlan.Step.Barrier.class, next.getFirst()).transitions(), "no transition, it is in GENERAL");
        assertEquals(new CommandPlan.Step.Fill<>("debug", false), next.get(1));
    }

    @Test
    void recreatedResourcesAreInitializedAgain() {
        ResourceInit<String> init = new ResourceInit<>();
        init.created("relative", false);
        steps(init, List.of("relative"), 1);
        init.destroyed("relative");
        assertEquals(3, steps(init, List.of("relative"), 2).size(), "untracked resources are left alone");
        init.created("relative", false);
        assertEquals(new CommandPlan.Step.Barrier<>(List.of("relative")), steps(init, List.of("relative"), 2).getFirst());
    }
}
